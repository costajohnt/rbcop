//! Prism lexer tokens shaped like rubocop-ast's `processed_source.sorted_tokens` (parser gem tokens).

use crate::source::{Source, Token};
use ruby_prism_sys as sys;

/// Prism tokens minus those the parser gem has no counterpart for: newlines the grammar ignores
/// (parser emits `tNL` only for significant ones), `__END__` (the translation lexer always skips
/// it) and the body/end of `=begin` blocks (parser emits one `tCOMMENT` for the whole block;
/// `EMBDOC_BEGIN` stands in for it).
/// Not modelled: parser's `tNL` after every comment and zero-width `tSPACE` closing `%w()`;
/// Prism's `COMMENT` includes the newline (parser's comment ends before it).
pub fn sorted_tokens(src: &Source) -> Vec<Token> {
    src.tokens()
        .iter()
        .filter(|t| {
            !matches!(
                t.kind,
                sys::PM_TOKEN_IGNORED_NEWLINE | sys::PM_TOKEN___END__ | sys::PM_TOKEN_EMBDOC_LINE | sys::PM_TOKEN_EMBDOC_END
            )
        })
        .copied()
        .collect()
}

/// `processed_source.tokens.empty?` (`first_code`: `cx.facts.first_code`). Code or a comment means
/// tokens without lexing; otherwise a lone `;` still counts, blank lines and `__END__` do not.
pub fn tokens_empty(src: &Source, first_code: Option<usize>) -> bool {
    first_code.is_none() && src.comments.is_empty() && sorted_tokens(src).is_empty()
}

/// Offset of `processed_source.tokens.find { |t| !t.comment? }` (`first_code`: `cx.facts.first_code`,
/// the first statement). Only a `;` can come before the first statement, so the lexer runs just
/// when the text before it has one.
pub fn first_non_comment_token(src: &Source, first_code: Option<usize>) -> Option<usize> {
    if !src.text[..first_code.unwrap_or(src.text.len())].contains(';') {
        return first_code;
    }
    let comment = |k: u32| matches!(k, sys::PM_TOKEN_COMMENT | sys::PM_TOKEN_EMBDOC_BEGIN | sys::PM_TOKEN_NEWLINE);
    sorted_tokens(src).iter().find(|t| !comment(t.kind)).map(|t| t.start).or(first_code)
}

/// Line of `processed_source.tokens.last`: the translation lexer's order is the lexer's, except
/// that a newline followed by comments is emitted after them (`comment_newline_location`), so a
/// file ending in comment lines ends with the last code line's `tNL`.
pub fn last_token_line(src: &Source) -> Option<usize> {
    use sys::*;
    let raw = src.lex_tokens();
    let skip = |k: u32| matches!(k, PM_TOKEN_IGNORED_NEWLINE | PM_TOKEN___END__ | PM_TOKEN_EMBDOC_LINE | PM_TOKEN_EMBDOC_END);
    let continuation = |t: Option<&Token>| {
        t.is_some_and(|t| matches!(t.kind, PM_TOKEN_COMMENT | PM_TOKEN_AMPERSAND_DOT | PM_TOKEN_DOT))
    };
    let (mut last, mut deferred) = (None, None);
    // Line of the last terminator of the heredocs opened on the current line (Prism lexes a
    // heredoc's body right after its opener).
    let mut heredoc_end = None;
    let lvar_ends = std::cell::OnceCell::new();
    for (i, t) in raw.iter().enumerate() {
        if t.kind == PM_TOKEN_HEREDOC_END {
            heredoc_end = Some(src.line_of(t.start));
        }
        if skip(t.kind) {
            if t.kind == PM_TOKEN_IGNORED_NEWLINE {
                heredoc_end = None;
            }
            continue;
        }
        let mut line = src.line_of(t.start);
        let next = raw.get(i + 1);
        if t.kind == PM_TOKEN_NEWLINE
            && let Some(h) = heredoc_end.take()
            && src.parser_gem
            && i > 0
            && legacy_arg_state(&raw[i - 1], || lvar_ends.get_or_init(|| lvar_read_ends(src)))
        {
            // The parser gem's lexer reads the newline of a line with heredocs in an argument
            // state twice: `on_newline` skips past the heredoc bodies, then `fhold; fgoto expr_end`
            // re-lexes the newline ending the last terminator line, where the tNL lands.
            line = h;
        }
        match t.kind {
            PM_TOKEN_NEWLINE if next.is_some_and(|n| n.kind == PM_TOKEN_COMMENT) => deferred = Some(line),
            PM_TOKEN_COMMENT => {
                let at_eol = !src.text[t.start..t.end].ends_with('\n');
                let inline = i > 0 && src.line_of(raw[i - 1].start) == line;
                last = Some(line);
                // An inline comment before code gets its own tNL right after it.
                let own_newline = inline && !at_eol && !continuation(next);
                if own_newline {
                    continue;
                }
                if inline && next.is_some_and(|n| n.kind == PM_TOKEN_COMMENT) {
                    deferred = Some(line);
                } else if let Some(d) = deferred.filter(|_| !continuation(next)) {
                    last = Some(d);
                    deferred = None;
                }
            }
            _ => last = Some(line),
        }
    }
    last
}

/// After this token the parser gem's lexer is in an argument state (expr_arg, expr_cmdarg,
/// expr_mid), where a newline is re-lexed in expr_end: a method name (an identifier that is not a
/// known local variable, a constant, `a.b`), `super`, `yield`, `defined?`, `return`, `break`, `next`.
/// Literals, variables, `)`, `]`, `}`, `self`/`nil`/`true` and a comment leave it in expr_end.
fn legacy_arg_state<'a>(t: &Token, lvar_ends: impl FnOnce() -> &'a Vec<usize>) -> bool {
    use sys::*;
    match t.kind {
        PM_TOKEN_IDENTIFIER => lvar_ends().binary_search(&t.end).is_err(),
        PM_TOKEN_CONSTANT
        | PM_TOKEN_METHOD_NAME
        | PM_TOKEN_KEYWORD_SUPER
        | PM_TOKEN_KEYWORD_YIELD
        | PM_TOKEN_KEYWORD_DEFINED
        | PM_TOKEN_KEYWORD_RETURN
        | PM_TOKEN_KEYWORD_BREAK
        | PM_TOKEN_KEYWORD_NEXT => true,
        _ => false,
    }
}

/// Sorted end offsets of the local variable reads (an identifier ending there is a declared local,
/// not a method call). One extra parse per file, only when a heredoc line ends in an identifier
/// below 3.3.
fn lvar_read_ends(src: &Source) -> Vec<usize> {
    use ruby_prism::Visit;
    struct Find(Vec<usize>);
    impl<'pr> Visit<'pr> for Find {
        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
            self.0.push(n.location().end_offset());
        }
    }
    let result = ruby_prism::parse_with_version(src.text.as_bytes(), src.version, !src.binary);
    let mut find = Find(Vec::new());
    find.visit(&result.node());
    find.0.sort_unstable();
    find.0
}

/// Position-sorted `tokens` (e.g. `translated`) in lexer order, the order of
/// `processed_source.tokens`: a heredoc's body follows its opening token. Each token takes the
/// lexer position of the raw token it starts in.
pub fn in_lex_order(src: &Source, tokens: &[Token]) -> Vec<Token> {
    let mut rank: Vec<(usize, usize)> = src.lex_tokens().iter().enumerate().map(|(i, t)| (t.start, i)).collect();
    rank.sort_unstable();
    let rank_of = |t: &Token| rank.partition_point(|&(s, _)| s <= t.start).checked_sub(1).map_or(0, |i| rank[i].1);
    let mut out = tokens.to_vec();
    out.sort_by_key(|t| (rank_of(t), t.start, t.end));
    out
}

/// Token#line equality (both tokens' begin lines).
pub fn same_line(src: &Source, a: &Token, b: &Token) -> bool {
    src.line_of(a.start) == src.line_of(b.start)
}

/// Token#left_curly_brace? (tLCURLY / tLAMBEG). Prism uses BRACE_LEFT for hash braces too.
pub fn left_curly_brace(t: &Token) -> bool {
    matches!(t.kind, sys::PM_TOKEN_BRACE_LEFT | sys::PM_TOKEN_LAMBDA_BEGIN)
}

/// Parser token types with no single Prism counterpart (see `translated`).
pub const T_OP_ASGN: u32 = 1000;
/// A whole simple string literal (`'a'`, `"a"`, `""`): parser's `tSTRING`.
pub const T_STRING: u32 = 1001;
/// `:sym` in one token: parser's `tSYMBOL`.
pub const T_SYMBOL: u32 = 1002;
/// Regexp options after the closing `/` (possibly empty): parser's `tREGEXP_OPT`.
pub const T_REGEXP_OPT: u32 = 1003;

/// `processed_source.tokens` as RuboCop sees them with the Prism translation lexer
/// (`Prism::Translation::Parser::Lexer#to_a`), in source order. Kinds are Prism kinds standing in
/// for the parser type (many-to-one groups collapsed, e.g. every `x=` operator is `T_OP_ASGN`),
/// plus the `T_*` constants above. Ported: `tNL` after inline comments, comments without their
/// newline, merged `tSTRING`/`tSYMBOL`, `tREGEXP_OPT`, heredoc ends without the newline, string
/// content split per line, `tSPACE` in percent arrays, `tUNARY_NUM` for `+1`.
/// Not ported: token type changes that need the lexer state (`tLBRACE` vs `tLCURLY`,
/// `tLPAREN` vs `tLPAREN2`, `kDO_LAMBDA`), and joining of continued single-line heredoc content.
pub fn translated(src: &Source) -> Vec<Token> {
    use sys::*;
    let raw = src.tokens();
    let text = src.text.as_bytes();
    let mut out: Vec<Token> = Vec::with_capacity(raw.len() + 16);
    let mut pending_nl: Option<Token> = None;
    let mut quotes: Vec<&[u8]> = Vec::new();
    let tok = |kind: u32, start: usize, end: usize| Token { kind, start, end };
    let mut i = 0;
    while i < raw.len() {
        let t = raw[i];
        i += 1;
        let next = raw.get(i).copied();
        let next_kind = next.map(|n| n.kind);
        let value = &text[t.start..t.end];
        match t.kind {
            PM_TOKEN_IGNORED_NEWLINE | PM_TOKEN___END__ | PM_TOKEN_EOF | PM_TOKEN_EMBDOC_LINE | PM_TOKEN_EMBDOC_END
            | PM_TOKEN_MISSING | PM_TOKEN_NOT_PROVIDED => {}
            // `def foo?`: the lexer's tFID right after `def` is a tIDENTIFIER.
            PM_TOKEN_METHOD_NAME if out.last().is_some_and(|l| l.kind == PM_TOKEN_KEYWORD_DEF) => {
                out.push(tok(PM_TOKEN_IDENTIFIER, t.start, t.end));
            }
            PM_TOKEN_EMBDOC_BEGIN => {
                let mut end = t.end;
                while i < raw.len() {
                    end = raw[i].end;
                    i += 1;
                    if raw[i - 1].kind == PM_TOKEN_EMBDOC_END {
                        break;
                    }
                }
                out.push(tok(PM_TOKEN_COMMENT, t.start, end));
            }
            PM_TOKEN_COMMENT => {
                let at_eol = !value.ends_with(b"\n");
                let comment = tok(PM_TOKEN_COMMENT, t.start, if at_eol { t.end } else { t.end - 1 });
                let inline = i >= 2 && src.line_of(raw[i - 2].start) == src.line_of(t.start);
                let continued = matches!(next_kind, Some(PM_TOKEN_COMMENT | PM_TOKEN_AMPERSAND_DOT | PM_TOKEN_DOT));
                out.push(comment);
                if inline && !at_eol && !continued {
                    out.push(tok(PM_TOKEN_NEWLINE, t.end - 1, t.end));
                } else if inline && next_kind == Some(PM_TOKEN_COMMENT) {
                    pending_nl = Some(tok(PM_TOKEN_NEWLINE, t.end - 1, t.end));
                } else if pending_nl.is_some() && !continued {
                    out.push(pending_nl.take().unwrap());
                }
            }
            PM_TOKEN_NEWLINE => {
                if next_kind == Some(PM_TOKEN_COMMENT) {
                    pending_nl = Some(t);
                } else {
                    out.push(t);
                }
            }
            PM_TOKEN_INTEGER if value.starts_with(b"+") => {
                out.push(tok(PM_TOKEN_UMINUS_NUM, t.start, t.start + 1));
                out.push(tok(PM_TOKEN_INTEGER, t.start + 1, t.end));
            }
            PM_TOKEN_STRING_BEGIN => {
                let basic = value == b"\"" || value == b"'";
                let after = raw.get(i + 1).map(|n| n.kind);
                if basic && next_kind == Some(PM_TOKEN_STRING_END) {
                    out.push(tok(T_STRING, t.start, next.unwrap().end));
                    i += 1;
                } else if matches!(value.first(), Some(b'\'' | b'"' | b'%'))
                    && next_kind == Some(PM_TOKEN_STRING_CONTENT)
                    && after == Some(PM_TOKEN_STRING_END)
                    && simplify_string(&text[next.unwrap().start..next.unwrap().end], value)
                {
                    out.push(tok(T_STRING, t.start, raw[i + 1].end));
                    i += 2;
                } else {
                    quotes.push(value);
                    out.push(t);
                }
            }
            PM_TOKEN_HEREDOC_START => {
                let q = match value.get(2) {
                    Some(b'-' | b'~') => value.get(3),
                    q => q,
                };
                quotes.push(match q {
                    Some(b'\'') => b"<<'",
                    Some(b'`') => b"<<`",
                    _ => b"<<\"",
                });
                out.push(tok(PM_TOKEN_STRING_BEGIN, t.start, t.end));
            }
            PM_TOKEN_STRING_CONTENT => {
                let quote = quotes.last().copied().unwrap_or(b"\"");
                let (interp, percent) = (interpolation(quote), percent_array(quote));
                let lines: Vec<&[u8]> = value.split_inclusive(|&c| c == b'\n').collect();
                let n = lines.len();
                let (mut start, mut pos) = (t.start, t.start);
                for (k, line) in lines.into_iter().enumerate() {
                    pos += line.len();
                    let chomped = line.strip_suffix(b"\n").unwrap_or(line);
                    let backslashes = chomped.iter().rev().take_while(|&&c| c == b'\\').count();
                    let continued = n > 1 && backslashes % 2 == 1 && (interp || percent);
                    if !continued || k == n - 1 {
                        out.push(tok(PM_TOKEN_STRING_CONTENT, start, pos));
                        start = pos;
                    }
                }
            }
            PM_TOKEN_STRING_END | PM_TOKEN_HEREDOC_END | PM_TOKEN_REGEXP_END => {
                let mut end_tok = tok(PM_TOKEN_STRING_END, t.start, t.end);
                if t.kind == PM_TOKEN_HEREDOC_END && value.ends_with(b"\n") {
                    end_tok.end -= if value.ends_with(b"\r\n") { 2 } else { 1 };
                } else if t.kind == PM_TOKEN_REGEXP_END {
                    end_tok.end = t.start + 1;
                }
                if quotes.pop().is_some_and(percent_array) {
                    let prev = (i >= 2).then(|| raw[i - 2].kind);
                    let empty = matches!(
                        prev,
                        Some(
                            PM_TOKEN_PERCENT_LOWER_I
                                | PM_TOKEN_PERCENT_LOWER_W
                                | PM_TOKEN_PERCENT_UPPER_I
                                | PM_TOKEN_PERCENT_UPPER_W
                        )
                    );
                    if !empty && prev != Some(PM_TOKEN_WORDS_SEP) {
                        out.push(tok(PM_TOKEN_WORDS_SEP, t.start, t.start));
                    }
                }
                out.push(end_tok);
                if t.kind == PM_TOKEN_REGEXP_END {
                    out.push(tok(T_REGEXP_OPT, t.start + 1, t.end));
                }
            }
            PM_TOKEN_WORDS_SEP => {
                let len = if value.first() == Some(&b'\n') {
                    1
                } else {
                    std::str::from_utf8(value)
                        .map_or(0, |s| s.chars().take_while(|&c| c != '\n').map(char::len_utf8).sum())
                };
                out.push(tok(PM_TOKEN_WORDS_SEP, t.start, t.start + len));
            }
            PM_TOKEN_SYMBOL_BEGIN => {
                if next.is_some_and(|n| {
                    !matches!(
                        n.kind,
                        PM_TOKEN_STRING_CONTENT | PM_TOKEN_EMBEXPR_BEGIN | PM_TOKEN_EMBVAR | PM_TOKEN_STRING_END
                    )
                }) {
                    out.push(tok(T_SYMBOL, t.start, next.unwrap().end));
                    i += 1;
                } else {
                    quotes.push(value);
                    out.push(t);
                }
            }
            PM_TOKEN_PERCENT_LOWER_I | PM_TOKEN_PERCENT_LOWER_W | PM_TOKEN_PERCENT_UPPER_I | PM_TOKEN_PERCENT_UPPER_W => {
                if next_kind == Some(PM_TOKEN_WORDS_SEP) {
                    i += 1;
                }
                quotes.push(value);
                out.push(t);
            }
            PM_TOKEN_REGEXP_BEGIN => {
                quotes.push(value);
                out.push(t);
            }
            PM_TOKEN_BACKTICK | PM_TOKEN_PERCENT_LOWER_X => {
                quotes.push(value);
                out.push(tok(PM_TOKEN_BACKTICK, t.start, t.end));
            }
            k => out.push(tok(canonical(k), t.start, t.end)),
        }
    }
    out
}

/// `translated`, stably sorted by position (rubocop-ast `sorted_tokens`).
pub fn translated_sorted(src: &Source) -> Vec<Token> {
    let mut t = translated(src);
    t.sort_by_key(|t| t.start);
    t
}

/// Prism kinds that share one parser token type, collapsed to one representative.
fn canonical(kind: u32) -> u32 {
    use sys::*;
    match kind {
        PM_TOKEN_AMPERSAND_AMPERSAND_EQUAL
        | PM_TOKEN_AMPERSAND_EQUAL
        | PM_TOKEN_CARET_EQUAL
        | PM_TOKEN_GREATER_GREATER_EQUAL
        | PM_TOKEN_LESS_LESS_EQUAL
        | PM_TOKEN_MINUS_EQUAL
        | PM_TOKEN_PERCENT_EQUAL
        | PM_TOKEN_PLUS_EQUAL
        | PM_TOKEN_PIPE_EQUAL
        | PM_TOKEN_PIPE_PIPE_EQUAL
        | PM_TOKEN_SLASH_EQUAL
        | PM_TOKEN_STAR_EQUAL
        | PM_TOKEN_STAR_STAR_EQUAL => T_OP_ASGN,
        PM_TOKEN_FLOAT_IMAGINARY | PM_TOKEN_FLOAT_RATIONAL_IMAGINARY | PM_TOKEN_INTEGER_RATIONAL_IMAGINARY => {
            PM_TOKEN_INTEGER_IMAGINARY
        }
        PM_TOKEN_FLOAT_RATIONAL => PM_TOKEN_INTEGER_RATIONAL,
        k => k,
    }
}

fn interpolation(quote: &[u8]) -> bool {
    !quote.ends_with(b"'") && ![&b"%q"[..], b"%w", b"%i", b"%s"].iter().any(|p| quote.starts_with(p))
}

fn percent_array(quote: &[u8]) -> bool {
    [&b"%w"[..], b"%W", b"%i", b"%I"].iter().any(|p| quote.starts_with(p))
}

/// Lexer#simplify_string?
fn simplify_string(value: &[u8], quote: &[u8]) -> bool {
    match quote {
        b"'" => !value.contains(&b'\n'),
        b"\"" => value.split_inclusive(|&c| c == b'\n').all(|line| {
            let Some(body) = line.strip_suffix(b"\n") else { return true };
            body.iter().rev().take_while(|&&c| c == b'\\').count() % 2 == 1
        }),
        _ => false,
    }
}

/// Ruby's `\s`: ASCII space, \t, \n, \v, \f, \r.
pub fn is_ruby_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Token#space_after? (`/\G\s/` at the token's end).
pub fn space_after(src: &Source, t: &Token) -> bool {
    src.text.as_bytes().get(t.end).is_some_and(|&c| is_ruby_space(c))
}

/// Token#space_before?
pub fn space_before(src: &Source, t: &Token) -> bool {
    src.text.as_bytes().get(t.start.saturating_sub(1)).is_some_and(|&c| is_ruby_space(c))
}

/// `processed_source.tokens_within(start...end)` over sorted tokens: from the first token starting
/// at or after `start` to the first token ending at or after `end`. As in Ruby, a missing first
/// token makes the slice start at 0 and a missing last token makes it run to the end.
pub fn within(tokens: &[Token], start: usize, end: usize) -> &[Token] {
    let n = tokens.len();
    let b = tokens.partition_point(|t| t.start < start);
    let b = if b == n { 0 } else { b };
    let e = tokens.partition_point(|t| t.end < end);
    let e = if e == n { n } else { e + 1 };
    if b >= e { &[] } else { &tokens[b..e] }
}
