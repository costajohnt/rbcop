//! Lint/Syntax for targets below Ruby 3.3, where RuboCop parses with the parser gem's grammar for the
//! target (Parser::Ruby20 ... Ruby32) instead of Prism. rbcop parses with Prism's 3.3 grammar, which
//! accepts newer syntax, so this finds the constructs the target's grammar rejects (one AST walk)
//! and reproduces the parser gem's diagnostics: `unexpected token <tok>` at the token where its LALR
//! parser fails, plus whichever later errors racc's error recovery lets through.
//!
//! Files Prism rejects too get the same treatment, seeded with Prism's first syntax error.
//!
//! Recovery model (racc + the grammar's `stmts: error stmt` / `top_stmts: error top_stmt`), checked
//! against the parser gem on probe files: on an error the parser pops to the innermost construct
//! whose statement body has begun, discards tokens until one that can start a statement, and parses
//! on from there as a new statement; a further error is reported only after three tokens have been
//! shifted since the last one. Constructs popped this way leave their closers unmatched
//! (`unexpected token kEND`, `$end`), and a popped def leaves the parser's in_def flag set (`class
//! definition in method body`, `dynamic constant assignment`). Where the resumed parse fails next
//! comes from re-parsing the rest of the file with Prism behind the open constructs' headers.
//! ponytail: an approximation of the LALR automaton, not a port of its tables; the remaining
//! differences are listed in docs/known-gaps.md.

use ruby_prism::{Node, Token, Visit};
use std::collections::{HashMap, HashSet};

/// (start, end, message) of each parser-gem diagnostic, before Lint/Syntax's location adjustment.
/// `seed`: where the source fails to parse even with Prism (see `failure_offset`); `actions`: end
/// offsets of the semantic errors (reported by the caller), whose yyerror also starts recovery;
/// `circular`: the reads of circular argument references, reported here unless recovery skips them.
pub fn diagnostics(
    root: &Node,
    prism_tokens: &[Token],
    text: &str,
    target: f64,
    seed: Option<usize>,
    actions: &[usize],
    circular: &[(usize, usize)],
) -> Vec<(usize, usize, String)> {
    let toks = convert(prism_tokens, text, target);
    let sorted = by_start(&toks);
    let mut w = Walk { target, toks: &sorted, ..Walk::default() };
    w.visit(root);
    // Before 2.7 a comment line ends the statement before a leading `.`/`&.` (`convert`), which
    // then cannot start one; Prism accepts the call, so there is no seed for it.
    if target < 2.7 {
        for p in sorted.windows(2).filter(|p| p[0].name == "tNL" && matches!(p[1].name, "tDOT" | "tAMPER")) {
            w.violations.entry(p[1].start).or_insert(p[1].start);
        }
    }
    if w.violations.is_empty() && seed.is_none() && circular.is_empty() && w.semantic.is_empty() {
        return Vec::new();
    }
    // The walk's builder errors, reported as they reduce: before the first grammar failure.
    let cut = w.violations.keys().copied().chain(seed).min().unwrap_or(usize::MAX);
    let semantic: Vec<_> = std::mem::take(&mut w.semantic).into_iter().filter(|s| s.0 < cut).collect();
    let mut actions = actions.to_vec();
    actions.extend(semantic.iter().map(|s| s.1));
    actions.sort_unstable();
    let mut out = simulate(&toks, &w, text, target, seed.unwrap_or(usize::MAX), &actions, circular);
    out.extend(semantic);
    out
}

/// `text` with the whitespace-only lines between a receiver and a leading `.`/`&.` (of the
/// `.`/`&.` tokens at `dots`) turned into comment lines, which Prism continues the call over, and
/// the offsets of those `#`s. The parser gem (2.7 to 3.2) skips such lines, but an empty line ends
/// the statement there too, so a dot after one keeps its error. Parse the copy with
/// `parse_with_version_and_tokens_restoring` so no slice of the AST reads a `#` (#169).
pub fn continue_over_blank_lines(text: &str, dots: &[usize]) -> Option<(Vec<u8>, Vec<usize>)> {
    let mut fake = Vec::new();
    for &dot in dots {
        if !text[dot..].starts_with('.') && !text[dot..].starts_with("&.") {
            continue;
        }
        let mut line_start = text[..dot].rfind('\n').map_or(0, |i| i + 1);
        if !text[line_start..dot].trim_start_matches([' ', '\t']).is_empty() {
            continue;
        }
        let mut blank = Vec::new();
        while line_start > 0 {
            let prev = text[..line_start - 1].rfind('\n').map_or(0, |i| i + 1);
            let line = &text[prev..line_start - 1];
            if line.is_empty() {
                blank.clear();
                break;
            }
            let rest = line.trim_start_matches([' ', '\t']);
            if rest.is_empty() {
                blank.push(prev);
            } else if !rest.starts_with('#') {
                break;
            }
            line_start = prev;
        }
        fake.extend(blank);
    }
    if fake.is_empty() {
        return None;
    }
    let mut bytes = text.as_bytes().to_vec();
    for &f in &fake {
        bytes[f] = b'#';
    }
    Some((bytes, fake))
}

/// The parser gem's lexer ends the input at `__END__` alone on a line it reads in its line_begin
/// state, after leading whitespace too; Prism (and Ruby) only at column 0, so for an indented one
/// returns its line start and indentation length (#228). Parse a copy with that line rewritten to
/// `__END__\n` + the indentation (same length), which Prism takes as the data marker.
///
/// line_begin follows a significant newline (or `;`, or the start of the file) and a newline the
/// lexer skips in its expr_value state: after a binary or assignment operator other than `=`, `?`,
/// `do`, a block's `{` or `|params|`, a def's `(`. After `=`, `,`, `(`, `[`, a hash's `{`, `.` or
/// `::` it reads `__END__` as an identifier.
// ponytail: an approximation of lexer.rl's transitions from Prism's lexer states, checked against
// RuboCop on the contexts above; a context not listed may still differ.
pub fn indented_data_marker(text: &str, tokens: &[Token]) -> Option<(usize, usize)> {
    let skipped = |k: &str| matches!(k, "COMMENT" | "IGNORED_NEWLINE" | "EMBDOC_BEGIN" | "EMBDOC_LINE" | "EMBDOC_END");
    tokens.iter().enumerate().filter(|(_, t)| t.kind == "IDENTIFIER" && &text[t.start..t.end] == "__END__").find_map(
        |(i, t)| {
            let line = text[..t.start].rfind('\n').map_or(0, |i| i + 1);
            let indent = &text[line..t.start];
            let prev = tokens[..i].iter().rposition(|p| !skipped(p.kind));
            // The `|` at `j` closes a block's parameters (`{ |a, b|`, `do |a|`).
            let block_params_end = |j: usize| {
                tokens[..j].iter().rposition(|x| x.kind == "PIPE").is_some_and(|k| {
                    k > 0 && matches!(tokens[k - 1].kind, "BRACE_LEFT" | "KEYWORD_DO" | "KEYWORD_DO_BLOCK")
                })
            };
            let line_begin = prev.is_none_or(|j| {
                let p = &tokens[j];
                let def_paren = p.kind == "PARENTHESIS_LEFT"
                    && tokens[j.saturating_sub(4)..j].iter().any(|d| d.kind == "KEYWORD_DEF");
                matches!(p.kind, "NEWLINE" | "SEMICOLON")
                    || p.state & NEWLINE_IGNORED == 0
                    || def_paren
                    || (p.kind == "PIPE" && block_params_end(j))
                    || (p.state & (EXPR_BEG | EXPR_LABEL) == EXPR_BEG
                        && (p.kind != "EQUAL" && !p.kind.ends_with("_EQUAL")
                            || matches!(
                                p.kind,
                                "EQUAL_EQUAL" | "EQUAL_EQUAL_EQUAL" | "BANG_EQUAL" | "LESS_EQUAL" | "GREATER_EQUAL"
                            ))
                        && !matches!(p.kind, "PIPE" | "TILDE" | "BANG" | "COMMA" | "BRACE_LEFT")
                        || p.kind == "BRACE_LEFT" && p.state & EXPR_LABEL == 0)
            });
            (!indent.is_empty()
                && indent.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\x0b' | b'\x0c'))
                && text[t.end..].starts_with('\n')
                && line_begin)
                .then_some((line, indent.len()))
        },
    )
}

/// NULs the parser gem's lexer ends a heredoc line at, where that makes a terminator line (Prism
/// reads them as text and leaves the heredoc open): in ` \0 SQL` or `  SQL\0` the segment between
/// NULs and newlines is the `SQL` terminator, its leading spaces and tabs allowed after `<<~`/`<<-`
/// (#247). Parse a copy with these NULs made newlines, written back once parsed.
pub fn heredoc_nul_breaks(text: &str, tokens: &[Token]) -> Vec<usize> {
    // Sets, so each segment costs two lookups however many heredocs the file opens (a file with many
    // heredocs and many NUL segments must stay linear).
    let (mut exact, mut indented): (HashSet<&str>, HashSet<&str>) = (HashSet::new(), HashSet::new());
    for t in tokens.iter().filter(|t| t.kind == "HEREDOC_START") {
        let Some(s) = text.get(t.start..t.end).and_then(|s| s.strip_prefix("<<")) else { continue };
        let (indent, s) = s.strip_prefix(['~', '-']).map_or((false, s), |r| (true, r));
        let id = s.trim_matches(['\'', '"', '`']);
        if indent {
            indented.insert(id)
        } else {
            exact.insert(id)
        };
    }
    let has_ids = !exact.is_empty() || !indented.is_empty();
    let mut out = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        if has_ids && body.contains('\0') {
            let mut seg_start = start;
            for seg in body.split('\0') {
                let seg_end = seg_start + seg.len();
                let closes = exact.contains(seg) || indented.contains(seg.trim_start_matches([' ', '\t']));
                if closes {
                    if seg_start > start {
                        out.push(seg_start - 1);
                    }
                    if seg_end < start + body.len() {
                        out.push(seg_end);
                    }
                }
                seg_start = seg_end + 1;
            }
        }
        start += line.len();
    }
    out
}

/// A Prism CONSTANT starting at `name` that the parser gem's lexer reads as an identifier (it
/// starts constants only at ASCII `A`-`Z`): its first character and that character's lowercase
/// form, which Prism reads as an identifier at the same offsets.
// ponytail: a same-length one-char lowercase only (else the constant stays); `Ж` and an existing
// `ж` would share one local.
pub fn non_ascii_constant_start(name: &str) -> Option<(char, char)> {
    let c = name.chars().next().filter(|c| !c.is_ascii())?;
    let mut lower = c.to_lowercase();
    let (Some(l), None) = (lower.next(), lower.next()) else { return None };
    (l != c && l.len_utf8() == c.len_utf8()).then_some((c, l))
}

/// Tokens in source order (the lexers emit a heredoc's body right after its opener).
fn by_start(toks: &[Tok]) -> Vec<Tok> {
    let mut sorted = toks.to_vec();
    sorted.sort_by_key(|t| t.start);
    sorted
}

/// Where the parser gem reports a semantic error Prism also finds: a circular argument reference
/// at the parameter's first read in its default value (Prism: at the parameter).
pub fn semantic_location(kind: &str, start: usize, end: usize, tokens: &[Token], text: &str) -> (usize, usize) {
    if matches!(kind, "class_name" | "module_name") {
        // The builder reports the cname token (`bar` of `class Foo::bar`).
        let name = tokens.iter().rfind(|t| t.start >= start && t.start < end.max(start + 1));
        return name.map_or((start, end), |t| (t.start, t.end));
    }
    if kind != "parameter_circular" {
        return (start, end);
    }
    let name = text.get(start..end).unwrap_or("");
    let mut rest =
        tokens.iter().filter(|t| t.start >= end && !matches!(t.kind, "COMMENT" | "IGNORED_NEWLINE")).peekable();
    while let Some(t) = rest.next() {
        let write = rest.peek().is_some_and(|n| matches!(parser_type(n.kind), Some("tEQL" | "tOP_ASGN")));
        if t.kind == "IDENTIFIER" && text.get(t.start..t.end) == Some(name) && !write {
            return (t.start, t.end);
        }
    }
    (start, end)
}

/// Builders::Default#static_regexp: the parser gem compiles a literal regexp with
/// `Regexp.new(source, EXTENDED if /x)` and reports the RegexpError (`empty char-class: /[]/`) on
/// the regexp. Prism's regexp_parse_error (at `at`) carries Onigmo's message; the source is the
/// literal's content with line continuations removed.
pub fn invalid_regexp(message: &str, at: usize, tokens: &[Token], text: &str) -> Option<(usize, usize, String)> {
    let i = tokens.iter().rposition(|t| t.kind == "REGEXP_BEGIN" && t.start <= at)?;
    let close = tokens[i..].iter().find(|t| t.kind == "REGEXP_END")?;
    let source = text.get(tokens[i].end..close.start)?.replace("\\\n", "");
    let extended = text.get(close.start..close.end)?.get(1..)?.contains('x');
    Some((tokens[i].start, close.end, format!("{message}: /{source}/{}", if extended { "x" } else { "" })))
}

/// Where the parser gem's lexer raises "unterminated string meets end of file": the opener of the
/// innermost string-like literal (string, symbol, regexp, xstring, %w/%i list) still open at the
/// end of the input. Prism reports some of these at the end of the file, others with their own
/// error kinds (`regexp_term`, `list_w_lower_term`, ...). Returns (outermost, innermost) opener:
/// Prism's errors from the outer literal left open are not the parser gem's.
/// `kinds` are Prism's error kinds; only one about a literal left open triggers the check.
pub fn unterminated_literal<'a>(mut kinds: impl Iterator<Item = &'a str>, tokens: &[Token]) -> Option<(usize, usize)> {
    let literal_eof = |k: &str| {
        matches!(
            k,
            "string_literal_eof" | "string_literal_term" | "string_interpolated_term" | "regexp_term" | "xstring_term"
        ) || k.starts_with("symbol_term_")
            || (k.starts_with("list_") && k.ends_with("_term"))
    };
    if !kinds.any(literal_eof) {
        return None;
    }
    let mut open: Vec<usize> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let next = tokens.get(i + 1).map_or("", |n| n.kind);
        match t.kind {
            // `:foo`, `:%`: a one-token symbol (no content or end follows its SYMBOL_BEGIN).
            "SYMBOL_BEGIN" if !matches!(next, "STRING_CONTENT" | "EMBEXPR_BEGIN" | "EMBVAR" | "STRING_END" | "EOF") => {
            }
            "STRING_BEGIN" | "SYMBOL_BEGIN" | "REGEXP_BEGIN" | "BACKTICK" | "PERCENT_LOWER_X" | "PERCENT_LOWER_W"
            | "PERCENT_UPPER_W" | "PERCENT_LOWER_I" | "PERCENT_UPPER_I" => open.push(t.start),
            "STRING_END" | "REGEXP_END" | "LABEL_END" => {
                open.pop();
            }
            "__END__" => break,
            _ => {}
        }
    }
    Some((*open.first()?, *open.last()?))
}

/// Prism's `class_name`/`module_name` when the parser gem fails the grammar instead of reporting
/// it from the builder: Some(offset of the failing token). The gem's `cpath` reaches the builder's
/// "must be a constant literal" check only for a `tIDENTIFIER` cname (`class foo`, `Foo::bar`,
/// `::foo`); a token that cannot start a name (`class <x`, `class;`) fails right after the
/// keyword, and any other primary (`class 1`, `class foo.bar`) fails where `::` was expected.
pub fn class_name_failure(kind: &str, start: usize, end: usize, tokens: &[Token]) -> Option<usize> {
    if !matches!(kind, "class_name" | "module_name") {
        return None;
    }
    let code = |t: &&Token| !matches!(t.kind, "COMMENT" | "NEWLINE" | "IGNORED_NEWLINE");
    let at_keyword = tokens.iter().any(|t| t.start == start && matches!(t.kind, "KEYWORD_CLASS" | "KEYWORD_MODULE"));
    if at_keyword {
        // The lexer skips newlines after `class`/`module` (EXPR_CLASS), not `;`.
        return tokens.iter().filter(code).find(|t| t.start >= end).map(|t| t.start);
    }
    let i = tokens.iter().rposition(|t| t.start < end.max(start + 1) && code(&t))?;
    let prev = tokens[..i].iter().rev().find(code).map(|t| t.kind);
    let cname = matches!(tokens[i].kind, "IDENTIFIER" | "CONSTANT")
        && prev.is_some_and(|k| matches!(k, "KEYWORD_CLASS" | "KEYWORD_MODULE" | "COLON_COLON" | "UCOLON_COLON"));
    if cname {
        return None;
    }
    tokens
        .iter()
        .find(|t| t.start >= tokens[i].end && !matches!(t.kind, "COMMENT" | "IGNORED_NEWLINE"))
        .map(|t| t.start)
}

/// The offset of the token an LALR parser fails on for a Prism syntax error: a write target
/// without `=` fails at the token after it, an unwritable expression at its `=`.
pub fn failure_offset(kind: &str, start: usize, end: usize, tokens: &[Token]) -> usize {
    // Input that ends inside a construct (`if`, `[1,`, `def (`): the token the parser cannot shift
    // is `$end`, for an error about what is missing there and for one located at the EOF token.
    let eof = tokens.last().map_or(start, |t| t.end);
    let only_trivia_from = |from: usize| {
        !tokens.iter().any(|t| {
            t.start >= from
                && !matches!(
                    t.kind,
                    "NEWLINE" | "IGNORED_NEWLINE" | "COMMENT" | "EOF" | "EMBDOC_BEGIN" | "EMBDOC_LINE" | "EMBDOC_END"
                )
        })
    };
    // Most errors sit on the construct missing something; these sit on the token that failed.
    let at_failing_token = kind.contains("unexpected")
        || kind.contains("_term")
        || kind.starts_with("expect_eol")
        || kind == "expect_expression_after_operator";
    let truncated =
        if at_failing_token { only_trivia_from(start) } else { !kind.starts_with("invalid_") && only_trivia_from(end) };
    if truncated {
        return eof;
    }
    if let Some(at) = class_name_failure(kind, start, end, tokens) {
        return at;
    }
    match kind {
        // Located at the `=`, `,`, ... (but `_after_operator` at the failing token); the parser
        // fails on what follows.
        "write_target_unexpected" => end,
        // A trailing `,` in parameters: `f_args` shifts it and fails on the `)` / `|` after it.
        "parameter_wild_loose_comma" => tokens
            .iter()
            .find(|t| t.start >= end && !matches!(t.kind, "NEWLINE" | "IGNORED_NEWLINE" | "COMMENT"))
            .map_or(end, |t| t.start),
        k if k.starts_with("expect_expression_after_") && k != "expect_expression_after_operator" => end,
        "ternary_expression_true" | "ternary_expression_false" => end,
        // A construct left open, reported at its keyword: the parser fails at the end of the file.
        "def_term" | "class_term" | "module_term" | "begin_term" | "case_term" | "for_term" | "while_term"
        | "until_term" | "conditional_term" | "block_term_end" | "lambda_term_end"
            if tokens.iter().any(|t| t.start == start && t.kind.starts_with("KEYWORD_")) =>
        {
            tokens.last().map_or(start, |t| t.end)
        }
        // Reported at the `{` whatever is wrong inside: at the latest the parser fails at `$end`.
        "hash_term" => eof,
        "expression_not_writable" => tokens
            .iter()
            .find(|t| t.start >= start && matches!(parser_type(t.kind), Some("tEQL" | "tOP_ASGN")))
            .map_or(start, |t| t.start),
        _ => start,
    }
}

/// Where the parse that resumes at `from` (inside the constructs whose headers are `prefix`) fails
/// first: the earliest Prism error or construct the target rejects, as an offset into `text`.
/// ponytail: one Prism parse of the rest of the file per resume point, capped by the caller.
fn first_error_after(
    text: &str,
    prefix: &str,
    from: usize,
    target: f64,
    skip: impl Fn(usize) -> bool,
    blank: &[(usize, usize)],
) -> (Option<usize>, Vec<usize>) {
    // Heredoc bodies whose opener is before `from` are text, not code: blanked (offsets kept).
    let mut rest = text[from..].to_string();
    for &(s, e) in blank {
        let range = s - from..e - from;
        let spaces: String = rest[range.clone()]
            .chars()
            .map(|c| if c == '\n' { "\n".to_string() } else { " ".repeat(c.len_utf8()) })
            .collect();
        rest.replace_range(range, &spaces);
    }
    let src = format!("{prefix}{rest}");
    let version = crate::config::target_ruby::prism_version(target);
    let (result, tokens) =
        ruby_prism::parse_with_version_and_tokens(src.as_bytes(), version, !crate::source::binary_encoding(text));
    let sorted = by_start(&convert(&tokens, &src, target));
    let mut w = Walk { target, toks: &sorted, ..Walk::default() };
    w.visit(&result.node());
    let p = prefix.len();
    // `return` directly in the class body recovery left open: the builder's error (and yyerror).
    let returns = result
        .errors()
        .filter(|e| e.type_name() == "return_invalid" && e.location().start_offset() >= p)
        .map(|e| e.location().start_offset() - p + from)
        .collect();
    // Unclosed constructs (errors at the end of the file) are the simulation's `$end` check;
    // semantic errors are not grammar failures (the in_def model covers dynamic constants), and
    // neither are `yield` outside a method or `else` without `rescue` (judged by the simulation).
    let errors = result
        .errors()
        .filter(|e| {
            let k = e.type_name();
            !k.contains("term")
                && (!crate::syntax::is_semantic(k)
                    || class_name_failure(k, e.location().start_offset(), e.location().end_offset(), &tokens).is_some())
                && !matches!(k, "invalid_yield" | "invalid_retry_without_rescue" | "return_invalid")
        })
        .map(|e| failure_offset(e.type_name(), e.location().start_offset(), e.location().end_offset(), &tokens))
        .filter(|&o| !src.get(o..).unwrap_or("").trim().is_empty())
        .collect::<Vec<_>>();
    let first = errors
        .into_iter()
        .chain(w.violations.keys().copied())
        .filter(|&o| o > p && !skip(o - p + from))
        .min()
        .map(|o| o - p + from);
    (first, returns)
}

/// A token as the parser gem's lexer for the target would produce it.
#[derive(Debug, Clone, Copy)]
struct Tok {
    name: &'static str,
    start: usize,
    end: usize,
    /// A `do` right after a command name (lexer state EXPR_CMDARG): kDO whatever cmdarg holds.
    cmd_do: bool,
}

/// Prism token type to parser gem token (Prism::Translation::Parser::Lexer::TYPES); None = skipped.
fn parser_type(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "AMPERSAND" => "tAMPER2",
        "AMPERSAND_AMPERSAND" => "tANDOP",
        "AMPERSAND_DOT" => "tANDDOT",
        "BACK_REFERENCE" => "tBACK_REF",
        "BACKTICK" | "PERCENT_LOWER_X" => "tXSTRING_BEG",
        "BANG" => "tBANG",
        "BANG_EQUAL" => "tNEQ",
        "BANG_TILDE" => "tNMATCH",
        "BRACE_LEFT" => "tLCURLY",
        "BRACE_RIGHT" => "tRCURLY",
        "BRACKET_LEFT" => "tLBRACK2",
        "BRACKET_LEFT_ARRAY" => "tLBRACK",
        "BRACKET_LEFT_RIGHT" => "tAREF",
        "BRACKET_LEFT_RIGHT_EQUAL" => "tASET",
        "BRACKET_RIGHT" => "tRBRACK",
        "CARET" => "tCARET",
        "CHARACTER_LITERAL" => "tCHARACTER",
        "CLASS_VARIABLE" => "tCVAR",
        "COLON" => "tCOLON",
        "COLON_COLON" => "tCOLON2",
        "COMMA" => "tCOMMA",
        "CONSTANT" => "tCONSTANT",
        "DOT" => "tDOT",
        "DOT_DOT" => "tDOT2",
        "DOT_DOT_DOT" => "tDOT3",
        "EMBEXPR_BEGIN" => "tSTRING_DBEG",
        "EMBEXPR_END" => "tSTRING_DEND",
        "EMBVAR" => "tSTRING_DVAR",
        "EQUAL" => "tEQL",
        "EQUAL_EQUAL" => "tEQ",
        "EQUAL_EQUAL_EQUAL" => "tEQQ",
        "EQUAL_GREATER" => "tASSOC",
        "EQUAL_TILDE" => "tMATCH",
        "FLOAT" => "tFLOAT",
        "FLOAT_IMAGINARY" | "FLOAT_RATIONAL_IMAGINARY" | "INTEGER_IMAGINARY" | "INTEGER_RATIONAL_IMAGINARY" => {
            "tIMAGINARY"
        }
        "FLOAT_RATIONAL" | "INTEGER_RATIONAL" => "tRATIONAL",
        "GLOBAL_VARIABLE" => "tGVAR",
        "GREATER" => "tGT",
        "GREATER_EQUAL" => "tGEQ",
        "GREATER_GREATER" => "tRSHFT",
        "HEREDOC_START" | "STRING_BEGIN" => "tSTRING_BEG",
        "HEREDOC_END" | "STRING_END" | "REGEXP_END" => "tSTRING_END",
        "IDENTIFIER" => "tIDENTIFIER",
        "INSTANCE_VARIABLE" => "tIVAR",
        "INTEGER" => "tINTEGER",
        "KEYWORD_ALIAS" => "kALIAS",
        "KEYWORD_AND" => "kAND",
        "KEYWORD_BEGIN" => "kBEGIN",
        "KEYWORD_BEGIN_UPCASE" => "klBEGIN",
        "KEYWORD_BREAK" => "kBREAK",
        "KEYWORD_CASE" => "kCASE",
        "KEYWORD_CLASS" => "kCLASS",
        "KEYWORD_DEF" => "kDEF",
        "KEYWORD_DEFINED" => "kDEFINED",
        "KEYWORD_DO" => "kDO",
        "KEYWORD_DO_LOOP" => "kDO_COND",
        "KEYWORD_END" => "kEND",
        "KEYWORD_END_UPCASE" => "klEND",
        "KEYWORD_ENSURE" => "kENSURE",
        "KEYWORD_ELSE" => "kELSE",
        "KEYWORD_ELSIF" => "kELSIF",
        "KEYWORD_FALSE" => "kFALSE",
        "KEYWORD_FOR" => "kFOR",
        "KEYWORD_IF" => "kIF",
        "KEYWORD_IF_MODIFIER" => "kIF_MOD",
        "KEYWORD_IN" => "kIN",
        "KEYWORD_MODULE" => "kMODULE",
        "KEYWORD_NEXT" => "kNEXT",
        "KEYWORD_NIL" => "kNIL",
        "KEYWORD_NOT" => "kNOT",
        "KEYWORD_OR" => "kOR",
        "KEYWORD_REDO" => "kREDO",
        "KEYWORD_RESCUE" => "kRESCUE",
        "KEYWORD_RESCUE_MODIFIER" => "kRESCUE_MOD",
        "KEYWORD_RETRY" => "kRETRY",
        "KEYWORD_RETURN" => "kRETURN",
        "KEYWORD_SELF" => "kSELF",
        "KEYWORD_SUPER" => "kSUPER",
        "KEYWORD_THEN" => "kTHEN",
        "KEYWORD_TRUE" => "kTRUE",
        "KEYWORD_UNDEF" => "kUNDEF",
        "KEYWORD_UNLESS" => "kUNLESS",
        "KEYWORD_UNLESS_MODIFIER" => "kUNLESS_MOD",
        "KEYWORD_UNTIL" => "kUNTIL",
        "KEYWORD_UNTIL_MODIFIER" => "kUNTIL_MOD",
        "KEYWORD_WHEN" => "kWHEN",
        "KEYWORD_WHILE" => "kWHILE",
        "KEYWORD_WHILE_MODIFIER" => "kWHILE_MOD",
        "KEYWORD_YIELD" => "kYIELD",
        "KEYWORD___ENCODING__" => "k__ENCODING__",
        "KEYWORD___FILE__" => "k__FILE__",
        "KEYWORD___LINE__" => "k__LINE__",
        "LABEL" => "tLABEL",
        "LABEL_END" => "tLABEL_END",
        "LAMBDA_BEGIN" => "tLAMBEG",
        "LESS" => "tLT",
        "LESS_EQUAL" => "tLEQ",
        "LESS_EQUAL_GREATER" => "tCMP",
        "LESS_LESS" => "tLSHFT",
        "METHOD_NAME" => "tFID",
        "MINUS" => "tMINUS",
        "MINUS_GREATER" => "tLAMBDA",
        "NEWLINE" => "tNL",
        "NUMBERED_REFERENCE" => "tNTH_REF",
        "PARENTHESIS_LEFT" => "tLPAREN2",
        "PARENTHESIS_LEFT_PARENTHESES" => "tLPAREN_ARG",
        "PARENTHESIS_RIGHT" => "tRPAREN",
        "PERCENT" => "tPERCENT",
        "PERCENT_LOWER_I" => "tQSYMBOLS_BEG",
        "PERCENT_LOWER_W" => "tQWORDS_BEG",
        "PERCENT_UPPER_I" => "tSYMBOLS_BEG",
        "PERCENT_UPPER_W" => "tWORDS_BEG",
        "PLUS" => "tPLUS",
        "PIPE" => "tPIPE",
        "PIPE_PIPE" => "tOROP",
        "QUESTION_MARK" => "tEH",
        "REGEXP_BEGIN" => "tREGEXP_BEG",
        "SEMICOLON" => "tSEMI",
        "SLASH" => "tDIVIDE",
        "STAR" => "tSTAR2",
        "STAR_STAR" => "tPOW",
        "STRING_CONTENT" => "tSTRING_CONTENT",
        "SYMBOL_BEGIN" => "tSYMBEG",
        "TILDE" => "tTILDE",
        "UAMPERSAND" => "tAMPER",
        "UCOLON_COLON" => "tCOLON3",
        "UDOT_DOT" => "tBDOT2",
        "UDOT_DOT_DOT" => "tBDOT3",
        "UMINUS" => "tUMINUS",
        "UMINUS_NUM" => "tUNARY_NUM",
        "UPLUS" => "tUPLUS",
        "USTAR" => "tSTAR",
        "USTAR_STAR" => "tDSTAR",
        "WORDS_SEP" => "tSPACE",
        k if k.ends_with("_EQUAL") => "tOP_ASGN",
        _ => return None,
    })
}

/// Token types after which the parser gem lexes `(` as tLPAREN (expression start).
const LPAREN_AFTER: &[&str] = &[
    "kBREAK",
    "tCARET",
    "kCASE",
    "tDIVIDE",
    "kFOR",
    "kIF",
    "kNEXT",
    "kRETURN",
    "kUNTIL",
    "kWHILE",
    "tAMPER",
    "tANDOP",
    "tBANG",
    "tCOMMA",
    "tDOT2",
    "tDOT3",
    "tEQL",
    "tLPAREN",
    "tLPAREN2",
    "tLPAREN_ARG",
    "tLSHFT",
    "tNL",
    "tOP_ASGN",
    "tOROP",
    "tPIPE",
    "tSEMI",
    "tSTRING_DBEG",
    "tUMINUS",
    "tUPLUS",
    "tLCURLY",
];

/// Lexer state bits (pm_lex_state_t, same values as Ripper's).
const EXPR_BEG: u32 = 0x1;
const EXPR_LABEL: u32 = 0x400;
/// EXPR_BEG | EXPR_MID | EXPR_CLASS.
const EXPR_BEG_ANY: u32 = 0x1 | 0x40 | 0x200;
const EXPR_CMDARG: u32 = 0x20;
/// States after which a newline is skipped (EXPR_BEG | EXPR_FNAME | EXPR_DOT | EXPR_CLASS).
const NEWLINE_IGNORED: u32 = 0x1 | 0x80 | 0x100 | 0x200;

/// Prism's tokens converted the way Prism::Translation::Parser::Lexer#to_a does for the parts the
/// grammar's error positions depend on (string/symbol merging, `(` and `{` kinds), followed by `$end`.
fn convert(tokens: &[Token], text: &str, target: f64) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len() + 1);
    // Lexer state after the previous token (the start of a file is EXPR_BEG).
    let mut state = EXPR_BEG;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        i += 1;
        if t.kind == "__END__" || t.kind == "EOF" {
            break;
        }
        let state_before = state;
        if !matches!(t.kind, "COMMENT" | "EMBDOC_BEGIN" | "EMBDOC_LINE" | "EMBDOC_END") {
            state = t.state;
        }
        // Prism folds a significant newline after a trailing comment into the COMMENT token; the
        // parser gem lexes tNL there (unless the next line continues with a leading `.`/`&.`).
        if t.kind == "COMMENT" && text[t.start..t.end].ends_with('\n') && state & NEWLINE_IGNORED == 0 {
            // Before 2.7 a comment line between the receiver and a leading `.`/`&.` ends the
            // statement (only the very next line can continue it).
            let next = if target < 2.7 {
                tokens.get(i).map_or("", |n| n.kind)
            } else {
                tokens[i..].iter().find(|n| n.kind != "COMMENT").map_or("", |n| n.kind)
            };
            // Blank lines up to the end of the file come out as IGNORED_NEWLINE tokens; the
            // comment's newline is still significant (`(a, # a\nb) # b\n\n`).
            let blank_to_eof = tokens[i..].iter().all(|n| matches!(n.kind, "COMMENT" | "IGNORED_NEWLINE" | "EOF"));
            if blank_to_eof || !matches!(next, "NEWLINE" | "IGNORED_NEWLINE" | "DOT" | "AMPERSAND_DOT") {
                out.push(Tok { name: "tNL", start: t.end - 1, end: t.end, cmd_do: false });
                state = EXPR_BEG;
            }
            continue;
        }
        // The newline Prism ignores before comment lines and a leading `.`/`&.` (2.7+ only).
        if target < 2.7
            && t.kind == "IGNORED_NEWLINE"
            && state_before & NEWLINE_IGNORED == 0
            && tokens.get(i).is_some_and(|n| n.kind == "COMMENT")
        {
            out.push(Tok { name: "tNL", start: t.start, end: t.end, cmd_do: false });
            state = EXPR_BEG;
            continue;
        }
        let Some(mut name) = parser_type(t.kind) else { continue };
        let mut end = t.end;
        // `&.` where a statement starts: the lexer reads `&` (tAMPER) and then `.`.
        if name == "tANDDOT" && out.last().is_none_or(|p| p.name == "tNL") {
            (name, end) = ("tAMPER", t.start + 1);
        }
        let next = |k: usize| tokens.get(k).map_or("", |t| t.kind);
        match name {
            "tSTRING_BEG" if t.kind == "STRING_BEGIN" => {
                let quote = text.get(t.start..t.end).unwrap_or("");
                if (quote == "'" || quote == "\"") && next(i) == "STRING_END" {
                    name = "tSTRING";
                    end = tokens[i].end;
                    i += 1;
                } else if (quote == "'" || quote == "\"") && next(i) == "STRING_CONTENT" && next(i + 1) == "STRING_END"
                {
                    let content = text.get(tokens[i].start..tokens[i].end).unwrap_or("");
                    // Lexer#simplify_string?: one line, or (double quotes) every line continued.
                    let simple = if quote == "'" {
                        !content.contains('\n')
                    } else {
                        content.split_inclusive('\n').all(|l| {
                            let body = l.trim_end_matches(['\n', '\r']);
                            !l.ends_with('\n') || (body.len() - body.trim_end_matches('\\').len()) % 2 == 1
                        })
                    };
                    if simple {
                        name = "tSTRING";
                        end = tokens[i + 1].end;
                        i += 2;
                    }
                }
            }
            "tSYMBEG" => {
                if !matches!(next(i), "STRING_CONTENT" | "EMBEXPR_BEGIN" | "EMBVAR" | "STRING_END" | "") {
                    name = "tSYMBOL";
                    end = tokens[i].end;
                    i += 1;
                }
            }
            // (Not after an operator method name: `alias % {`.)
            "tLCURLY"
                if t.state == EXPR_BEG | EXPR_LABEL
                    && !out.len().checked_sub(2).is_some_and(|k| matches!(out[k].name, "kALIAS" | "kUNDEF")) =>
            {
                name = "tLBRACE"
            }
            // The translator's list misses keywords like `in`; an expression-start lexer state
            // gives tLPAREN as well.
            "tLPAREN2"
                if out.last().is_none_or(|p| LPAREN_AFTER.contains(&p.name)) || state_before & EXPR_BEG_ANY != 0 =>
            {
                name = "tLPAREN"
            }
            "tFID" if out.last().is_some_and(|p| p.name == "kDEF") => name = "tIDENTIFIER",
            // A method name (`def for`, `x.class`, `A::end`): it opens or closes nothing.
            // ponytail: the parser gem keeps the keyword token after `def` (`fname: reswords`).
            k if k.starts_with('k')
                && out.last().is_some_and(|p| matches!(p.name, "kDEF" | "tDOT" | "tANDDOT" | "tCOLON2")) =>
            {
                name = "tIDENTIFIER"
            }
            "kDO" => {
                let lambda = out.iter().rev().find(|p| matches!(p.name, "kDO_LAMBDA" | "tLAMBDA" | "tLAMBEG"));
                if lambda.is_some_and(|p| p.name == "tLAMBDA") {
                    name = "kDO_LAMBDA";
                }
            }
            // Beginless-range tokens exist from 2.7, and only at the start of an expression
            // (`def foo ...` lexes tDOT3).
            "tBDOT2" if target < 2.7 || state_before & EXPR_BEG_ANY == 0 => name = "tDOT2",
            "tBDOT3" if target < 2.7 || state_before & EXPR_BEG_ANY == 0 => name = "tDOT3",
            _ => {}
        }
        // The parser gem's expr_cmdarg: a name lexed at a command start (file start, after a
        // newline, `;`, `(`, a block opener or a statement keyword; not after `|params|`, where
        // Prism's lexer state says EXPR_CMDARG too).
        let cmd_do = name == "kDO"
            && state_before & EXPR_CMDARG != 0
            && matches!(out.last().map(|p| p.name), Some("tIDENTIFIER" | "tFID" | "tCONSTANT"))
            && out.len().checked_sub(2).map(|k| out[k].name).is_none_or(|p| {
                matches!(
                    p,
                    "tNL"
                        | "tSEMI"
                        | "kDO"
                        | "kDO_BLOCK"
                        | "kDO_LAMBDA"
                        | "tLCURLY"
                        | "tLAMBEG"
                        | "tLPAREN"
                        | "tLPAREN2"
                        | "tLPAREN_ARG"
                        | "kTHEN"
                        | "kELSE"
                        | "kBEGIN"
                        | "kENSURE"
                        | "kIF"
                        | "kUNLESS"
                        | "kWHILE"
                        | "kUNTIL"
                        | "kAND"
                        | "kOR"
                        | "kNOT"
                        | "kRETURN"
                )
            });
        out.push(Tok { name, start: t.start, end, cmd_do });
        // A label ending its line: the newline is a tNL when the parser's in_kwarg flag is set
        // (see `simulate`), else skipped.
        if name == "tLABEL" {
            let rest = &text[end..];
            let line = &rest[..rest.find('\n').unwrap_or(rest.len())];
            let code = line.find('#').map_or(line, |c| &line[..c]);
            if code.trim().is_empty() && end + line.len() < text.len() {
                let nl = end + line.len();
                out.push(Tok { name: "tLABEL_NL", start: nl, end: nl + 1, cmd_do: false });
            }
        }
    }
    out.push(Tok { name: "$end", start: text.len(), end: text.len(), cmd_do: false });
    out
}

/// (start, end, message) of a diagnostic.
pub type Diagnostic = (usize, usize, String);

/// The parser gem's lexer on the first heredoc Prism reports unterminated (`heredoc_term`; `open`
/// holds the error offsets): (offset from which Prism-based diagnostics are dropped, the
/// diagnostics the lexer gives instead). Its heredoc rule needs a newline after the identifier, so
/// on a last line without one `<<'EOS'` is "unterminated heredoc id", after which the closing quote
/// opens a string that meets the end of the file; `<<'EOS` stops lexing (`$end` is unexpected
/// where a value is); a bare `<<EOS` is tLSHFT, unexpected where a value is. With a newline the
/// body runs to the end of the file: the fatal string_eof at the opener.
// ponytail: a bare `<<EOS` after a value (`foo <<EOS` at the end of a file) is a valid shift in
// the parser gem, which builds another AST; that case is left to the Prism-based diagnostics.
pub fn unterminated_heredoc(tokens: &[Token], text: &str, open: &[usize]) -> Option<(usize, Vec<Diagnostic>)> {
    let i =
        tokens.iter().position(|t| t.kind == "HEREDOC_START" && open.iter().any(|&o| o >= t.start && o <= t.end))?;
    let (s, e) = (tokens[i].start, tokens[i].end);
    let eof = |msg: &str, at: usize| (at, at + 1, msg.to_string());
    if text[e..].contains('\n') {
        return Some((s, vec![eof("unterminated string meets end of file", s)]));
    }
    let wants_value = tokens[..i]
        .iter()
        .rev()
        .find(|t| !matches!(t.kind, "COMMENT" | "IGNORED_NEWLINE" | "EMBDOC_BEGIN" | "EMBDOC_LINE" | "EMBDOC_END"))
        .is_none_or(|t| t.state & EXPR_BEG_ANY != 0);
    let opener = text.get(s..e).unwrap_or("");
    let id = opener.trim_start_matches('<').trim_start_matches(['-', '~']);
    let Some(quote) = id.chars().next().filter(|c| matches!(c, '\'' | '"' | '`')) else {
        return wants_value.then(|| (s, vec![(s, s + 2, "unexpected token tLSHFT".to_string())]));
    };
    let mut out = vec![eof("unterminated heredoc id", s)];
    if id.len() > 1 && id.ends_with(quote) {
        out.push(eof("unterminated string meets end of file", e - 1));
    } else if wants_value {
        out.push((text.len().saturating_sub(1), text.len(), "unexpected token $end".to_string()));
    }
    Some((s, out))
}

/// Tokens a statement can start with (FIRST(stmt) in the parser gem's grammars): where racc resumes
/// after discarding tokens.
fn starts_stmt(name: &str) -> bool {
    matches!(
        name,
        "tIDENTIFIER"
            | "tFID"
            | "tCONSTANT"
            | "tIVAR"
            | "tGVAR"
            | "tCVAR"
            | "tBACK_REF"
            | "tNTH_REF"
            | "tINTEGER"
            | "tFLOAT"
            | "tRATIONAL"
            | "tIMAGINARY"
            | "tCHARACTER"
            | "tSTRING"
            | "tSTRING_BEG"
            | "tXSTRING_BEG"
            | "tREGEXP_BEG"
            | "tWORDS_BEG"
            | "tQWORDS_BEG"
            | "tSYMBOLS_BEG"
            | "tQSYMBOLS_BEG"
            | "tSYMBEG"
            | "tSYMBOL"
            | "tLPAREN"
            | "tLPAREN_ARG"
            | "tLBRACK"
            | "tLBRACE"
            | "tLAMBDA"
            | "tCOLON3"
            | "tUMINUS"
            | "tUNARY_NUM"
            | "tUPLUS"
            | "tBANG"
            | "tTILDE"
            | "tSTAR"
            | "tBDOT2"
            | "tBDOT3"
            | "kALIAS"
            | "kUNDEF"
            | "klBEGIN"
            | "klEND"
            | "kBEGIN"
            | "kIF"
            | "kUNLESS"
            | "kWHILE"
            | "kUNTIL"
            | "kCASE"
            | "kFOR"
            | "kDEF"
            | "kCLASS"
            | "kMODULE"
            | "kRETURN"
            | "kBREAK"
            | "kNEXT"
            | "kREDO"
            | "kRETRY"
            | "kYIELD"
            | "kSUPER"
            | "kSELF"
            | "kNIL"
            | "kTRUE"
            | "kFALSE"
            | "kNOT"
            | "kDEFINED"
            | "k__FILE__"
            | "k__LINE__"
            | "k__ENCODING__"
    )
}

/// The token that closes a construct opened by `name`, if `name` opens one.
fn closer_of(name: &str) -> Option<&'static str> {
    Some(match name {
        "tLPAREN" | "tLPAREN2" | "tLPAREN_ARG" => "tRPAREN",
        "tLBRACK" | "tLBRACK2" => "tRBRACK",
        "tLBRACE" | "tLCURLY" | "tLAMBEG" => "tRCURLY",
        "tSTRING_DBEG" => "tSTRING_DEND",
        "kDEF" | "kCLASS" | "kMODULE" | "kIF" | "kUNLESS" | "kWHILE" | "kUNTIL" | "kCASE" | "kFOR" | "kBEGIN"
        | "kDO" | "kDO_LAMBDA" => "kEND",
        _ => return None,
    })
}

/// Tokens that end an expression.
fn ends_expr(name: &str) -> bool {
    matches!(
        name,
        "tIDENTIFIER"
            | "tFID"
            | "tCONSTANT"
            | "tIVAR"
            | "tGVAR"
            | "tCVAR"
            | "tBACK_REF"
            | "tNTH_REF"
            | "tINTEGER"
            | "tFLOAT"
            | "tRATIONAL"
            | "tIMAGINARY"
            | "tCHARACTER"
            | "tSTRING"
            | "tSTRING_END"
            | "tSYMBOL"
            | "tRPAREN"
            | "tRBRACK"
            | "tRCURLY"
            | "kEND"
            | "kSELF"
            | "kNIL"
            | "kTRUE"
            | "kFALSE"
            | "kREDO"
            | "kRETRY"
            | "k__FILE__"
            | "k__LINE__"
            | "k__ENCODING__"
            | "tLABEL_END"
    )
}

/// Tokens after which the end of the file is unexpected (`a.`, `a +`, `a,`).
fn wants_operand(name: &str) -> bool {
    matches!(
        name,
        "tDOT"
            | "tANDDOT"
            | "tCOLON2"
            | "tCOMMA"
            | "tEQL"
            | "tOP_ASGN"
            | "tANDOP"
            | "tOROP"
            | "kAND"
            | "kOR"
            | "tPLUS"
            | "tMINUS"
            | "tSTAR2"
            | "tDIVIDE"
            | "tPOW"
            | "tPERCENT"
            | "tPIPE"
            | "tAMPER2"
            | "tCARET"
            | "tLSHFT"
            | "tRSHFT"
            | "tLT"
            | "tGT"
            | "tLEQ"
            | "tGEQ"
            | "tEQ"
            | "tNEQ"
            | "tEQQ"
            | "tMATCH"
            | "tNMATCH"
            | "tCMP"
            | "tASSOC"
            | "tEH"
    )
}

fn is_closer(name: &str) -> bool {
    matches!(name, "tRPAREN" | "tRBRACK" | "tRCURLY" | "tSTRING_DEND" | "kEND")
}

#[derive(Default)]
struct Walk<'t> {
    target: f64,
    toks: &'t [Tok],
    /// Start offsets of the tokens the target's grammar fails on -> start of the construct that
    /// makes them fail (after error recovery resumes inside it, the parse differs and they do not).
    violations: HashMap<usize, usize>,
    /// Starts of the enclosing constructs violations belong to (call name, `[`, `{`, `def`, ...).
    ctx: Vec<usize>,
    /// Opener token start -> offset from which an error stays inside the construct (its statement
    /// body has begun, so racc finds an `error` shift there).
    body: HashMap<usize, usize>,
    /// `def` keywords of endless methods (no `end` to match).
    endless: HashSet<usize>,
    /// Mid keyword (`then`, `else`, `when`, `in`, `rescue`, `ensure`, `elsif`, loop `do`) -> opener of
    /// the construct it belongs to.
    owner: HashMap<usize, usize>,
    /// `...` call argument -> the forwarding `def` (3.0+ grammars check the def declares it).
    forwarding: HashMap<usize, usize>,
    /// Openers of the enclosing constructs that own `rescue`/`else`/`ensure` clauses.
    owners: Vec<usize>,
    /// Enclosing `def`s: (keyword offset, declares `...`).
    defs: Vec<(usize, bool)>,
    /// Constant assignment operator -> the target's range (`dynamic constant assignment` in a def).
    const_writes: HashMap<usize, (usize, usize)>,
    /// `do` keywords that are kDO_BLOCK.
    do_block: HashSet<usize>,
    /// `{` keywords the lexer gives as tLBRACE_ARG (a block on an index call: `a[k] {`).
    brace_arg: HashSet<usize>,
    /// Openers whose header takes the term before the body (`class A < B` + newline, `def a b` +
    /// newline, `if a` + newline / `then`, ...): right after it the parser is in the body's first
    /// state, where `stmts: error stmt` starts.
    term_headers: HashSet<usize>,
    /// Start -> end of command-call arguments (`foo a, b`), around which the parser pushes cmdarg.
    cmd_args: HashMap<usize, usize>,
    /// Heredocs: (opener start, body and terminator range).
    heredocs: Vec<(usize, (usize, usize))>,
    /// Builder errors the target's parser gem reports where Prism does not: (start, end, message).
    semantic: Vec<(usize, usize, String)>,
}

impl Walk<'_> {
    /// The first token after the one ending at `end`, skipping newlines (ignored after an operator by
    /// the older lexers).
    fn next_token(&self, end: usize) -> usize {
        let i = self.toks.partition_point(|t| t.start < end);
        self.toks[i..].iter().find(|t| !matches!(t.name, "tNL" | "tLABEL_NL")).map_or(usize::MAX, |t| t.start)
    }
    /// End of the first newline or `;` at or after `offset` (where a header without `then`/`do`/parens ends).
    fn term_after(&self, offset: usize) -> usize {
        let i = self.toks.partition_point(|t| t.start < offset);
        self.toks[i..].iter().find(|t| matches!(t.name, "tNL" | "tSEMI")).map_or(usize::MAX, |t| t.end)
    }
    fn violation(&mut self, offset: usize, below: f64) {
        if self.target < below {
            let ctx = self.ctx.last().copied().unwrap_or(offset);
            self.violations.entry(offset).or_insert(ctx);
        }
    }
    fn token_at(&self, offset: usize) -> Option<&Tok> {
        self.toks.get(self.toks.partition_point(|t| t.start < offset)).filter(|t| t.start == offset)
    }
    /// Builders::Default#check_condition: a multiple assignment as a condition is an error up to
    /// Ruby 2.3 (`if (a, b = foo)`), also inside `and`/`or`/`!`/parentheses/flip-flops.
    fn check_condition(&mut self, cond: &Node) {
        if let Some(m) = cond.as_multi_write_node() {
            if self.target < 2.4 {
                let l = m.location();
                let msg = "multiple assignment in conditional context".to_string();
                self.semantic.push((l.start_offset(), l.end_offset(), msg));
            }
        } else if let Some(b) = cond.as_parentheses_node().and_then(|p| p.body()) {
            self.check_condition(&b);
        } else if let Some(b) = cond.as_statements_node() {
            let body = b.body();
            if body.len() == 1 {
                self.check_condition(&body.iter().next().unwrap());
            }
        } else if let Some(a) = cond.as_and_node() {
            self.check_condition(&a.left());
            self.check_condition(&a.right());
        } else if let Some(o) = cond.as_or_node() {
            self.check_condition(&o.left());
            self.check_condition(&o.right());
        } else if let Some(r) = cond.as_range_node() {
            for side in [r.left(), r.right()].into_iter().flatten() {
                self.check_condition(&side);
            }
        } else if let Some(r) = cond.as_call_node().filter(|c| c.name().as_slice() == b"!").and_then(|c| c.receiver()) {
            self.check_condition(&r);
        }
    }
    fn in_ctx(&mut self, start: usize, f: impl FnOnce(&mut Self)) {
        self.ctx.push(start);
        f(self);
        self.ctx.pop();
    }
    fn mid(&mut self, kw: usize) {
        if let Some(&o) = self.owners.last() {
            self.owner.insert(kw, o);
        }
    }
    fn begin_clauses(&mut self, n: &ruby_prism::BeginNode) {
        let mut rescue = n.rescue_clause();
        while let Some(r) = rescue {
            self.mid(r.keyword_loc().start_offset());
            if let Some(t) = r.then_keyword_loc() {
                self.mid(t.start_offset());
            }
            rescue = r.subsequent();
        }
        if let Some(e) = n.else_clause() {
            self.mid(e.else_keyword_loc().start_offset());
        }
        if let Some(e) = n.ensure_clause() {
            self.mid(e.ensure_keyword_loc().start_offset());
        }
    }
    fn with_owner(&mut self, opener: usize, body: usize, f: impl FnOnce(&mut Self)) {
        self.body.insert(opener, body);
        self.owners.push(opener);
        f(self);
        self.owners.pop();
    }
}

impl<'pr> Visit<'pr> for Walk<'_> {
    fn visit_def_node(&mut self, n: &ruby_prism::DefNode<'pr>) {
        let kw = n.def_keyword_loc().start_offset();
        self.ctx.push(kw);
        let params = n.parameters();
        if n.equal_loc().is_some() && self.target >= 3.0 {
            self.endless.insert(kw);
            // 3.0 takes only an `arg` as the body: a command call fails at its first argument.
            if (3.0..3.1).contains(&self.target) {
                let body = n.body().and_then(|b| b.as_statements_node()).and_then(|s| s.body().iter().next());
                if let Some(c) = body.as_ref().and_then(|b| b.as_call_node()) {
                    let name = String::from_utf8_lossy(c.name().as_slice()).into_owned();
                    let command = c.opening_loc().is_none() && !crate::helpers::node_types::is_operator_method(&name);
                    if let Some(a) = c.arguments().and_then(|a| a.arguments().iter().next()).filter(|_| command) {
                        self.violation(a.location().start_offset(), 3.1);
                    }
                }
            }
        } else {
            // Below 3.0 an endless def fails at its `=`; with parentheses its body has begun there,
            // so recovery stays inside a def that then lacks its `end`.
            if let Some(eq) = n.equal_loc() {
                self.violation(eq.start_offset(), 3.0);
            }
            if n.lparen_loc().is_none() {
                self.term_headers.insert(kw);
            }
            let header_end = params.as_ref().map_or(n.name_loc().end_offset(), |p| p.location().end_offset());
            let body = n.rparen_loc().map_or_else(
                || if n.equal_loc().is_some() { usize::MAX } else { self.term_after(header_end) },
                |r| r.end_offset(),
            );
            self.body.insert(kw, body);
        }
        let forwarding =
            params.as_ref().and_then(|p| p.keyword_rest().filter(|k| k.as_forwarding_parameter_node().is_some()));
        if let (Some(p), Some(f)) = (&params, &forwarding) {
            let at = f.location().start_offset();
            // `...` alone from 2.7; after required parameters from 2.7 (parser gem); without
            // parentheses or after optional, rest or keyword parameters from 3.1.
            self.violation(at, 2.7);
            let unparenthesized = n.lparen_loc().is_none();
            if unparenthesized
                || p.optionals().iter().next().is_some()
                || p.rest().is_some()
                || p.keywords().iter().next().is_some()
            {
                self.violation(at, 3.1);
            }
        }
        self.defs.push((kw, forwarding.is_some()));
        self.owners.push(kw);
        if let Some(b) = n.body().and_then(|b| b.as_begin_node()) {
            self.begin_clauses(&b);
        }
        ruby_prism::visit_def_node(self, n);
        self.owners.pop();
        self.defs.pop();
        self.ctx.pop();
    }

    fn visit_class_node(&mut self, n: &ruby_prism::ClassNode<'pr>) {
        let kw = n.class_keyword_loc().start_offset();
        let header = n.superclass().map_or(n.constant_path().location().end_offset(), |s| s.location().end_offset());
        let body = self.term_after(header);
        if n.superclass().is_some() {
            self.term_headers.insert(kw);
        }
        self.with_owner(kw, body, |s| {
            if let Some(b) = n.body().and_then(|b| b.as_begin_node()) {
                s.begin_clauses(&b);
            }
            ruby_prism::visit_class_node(s, n);
        });
    }

    fn visit_module_node(&mut self, n: &ruby_prism::ModuleNode<'pr>) {
        let kw = n.module_keyword_loc().start_offset();
        let body = self.term_after(n.constant_path().location().end_offset());
        self.with_owner(kw, body, |s| {
            if let Some(b) = n.body().and_then(|b| b.as_begin_node()) {
                s.begin_clauses(&b);
            }
            ruby_prism::visit_module_node(s, n);
        });
    }

    fn visit_singleton_class_node(&mut self, n: &ruby_prism::SingletonClassNode<'pr>) {
        let kw = n.class_keyword_loc().start_offset();
        let body = self.term_after(n.expression().location().end_offset());
        self.term_headers.insert(kw);
        self.with_owner(kw, body, |s| {
            if let Some(b) = n.body().and_then(|b| b.as_begin_node()) {
                s.begin_clauses(&b);
            }
            ruby_prism::visit_singleton_class_node(s, n);
        });
    }

    fn visit_if_node(&mut self, n: &ruby_prism::IfNode<'pr>) {
        self.check_condition(&n.predicate());
        // Ternaries and modifiers have no `end`; an `elsif` belongs to the first `if`.
        let Some(kw) = n.if_keyword_loc().filter(|k| n.end_keyword_loc().is_some() && k.as_slice() == b"if") else {
            return ruby_prism::visit_if_node(self, n);
        };
        let kw = kw.start_offset();
        let body = n
            .then_keyword_loc()
            .map_or_else(|| self.term_after(n.predicate().location().end_offset()), |t| t.end_offset());
        self.body.insert(kw, body);
        self.term_headers.insert(kw);
        let mut cur = n.then_keyword_loc().map(|t| t.start_offset()).into_iter().collect::<Vec<_>>();
        let mut sub = n.subsequent();
        while let Some(s) = sub {
            if let Some(e) = s.as_else_node() {
                cur.push(e.else_keyword_loc().start_offset());
                sub = None;
            } else if let Some(i) = s.as_if_node() {
                cur.extend(i.if_keyword_loc().map(|l| l.start_offset()));
                cur.extend(i.then_keyword_loc().map(|l| l.start_offset()));
                sub = i.subsequent();
            } else {
                sub = None;
            }
        }
        for c in cur {
            self.owner.insert(c, kw);
        }
        ruby_prism::visit_if_node(self, n);
    }

    fn visit_unless_node(&mut self, n: &ruby_prism::UnlessNode<'pr>) {
        self.check_condition(&n.predicate());
        if n.end_keyword_loc().is_some() {
            let kw = n.keyword_loc().start_offset();
            let body = n
                .then_keyword_loc()
                .map_or_else(|| self.term_after(n.predicate().location().end_offset()), |t| t.end_offset());
            self.body.insert(kw, body);
            self.term_headers.insert(kw);
            for c in n.then_keyword_loc().into_iter().chain(n.else_clause().map(|e| e.else_keyword_loc())) {
                self.owner.insert(c.start_offset(), kw);
            }
        }
        ruby_prism::visit_unless_node(self, n);
    }

    fn visit_while_node(&mut self, n: &ruby_prism::WhileNode<'pr>) {
        self.check_condition(&n.predicate());
        if n.closing_loc().is_some() {
            let kw = n.keyword_loc().start_offset();
            let body = n
                .do_keyword_loc()
                .map_or_else(|| self.term_after(n.predicate().location().end_offset()), |d| d.end_offset());
            self.body.insert(kw, body);
            self.term_headers.insert(kw);
            if let Some(d) = n.do_keyword_loc() {
                self.owner.insert(d.start_offset(), kw);
            }
        }
        ruby_prism::visit_while_node(self, n);
    }

    fn visit_until_node(&mut self, n: &ruby_prism::UntilNode<'pr>) {
        self.check_condition(&n.predicate());
        if n.closing_loc().is_some() {
            let kw = n.keyword_loc().start_offset();
            let body = n
                .do_keyword_loc()
                .map_or_else(|| self.term_after(n.predicate().location().end_offset()), |d| d.end_offset());
            self.body.insert(kw, body);
            self.term_headers.insert(kw);
            if let Some(d) = n.do_keyword_loc() {
                self.owner.insert(d.start_offset(), kw);
            }
        }
        ruby_prism::visit_until_node(self, n);
    }

    fn visit_for_node(&mut self, n: &ruby_prism::ForNode<'pr>) {
        let kw = n.for_keyword_loc().start_offset();
        let body = n
            .do_keyword_loc()
            .map_or_else(|| self.term_after(n.collection().location().end_offset()), |d| d.end_offset());
        self.body.insert(kw, body);
        self.term_headers.insert(kw);
        self.owner.insert(n.in_keyword_loc().start_offset(), kw);
        if let Some(d) = n.do_keyword_loc() {
            self.owner.insert(d.start_offset(), kw);
        }
        ruby_prism::visit_for_node(self, n);
    }

    fn visit_case_node(&mut self, n: &ruby_prism::CaseNode<'pr>) {
        let kw = n.case_keyword_loc().start_offset();
        let mut first = true;
        for c in n.conditions().iter() {
            let Some(w) = c.as_when_node() else { continue };
            self.owner.insert(w.keyword_loc().start_offset(), kw);
            if let Some(t) = w.then_keyword_loc() {
                self.owner.insert(t.start_offset(), kw);
            }
            if first {
                let last =
                    w.conditions().iter().last().map_or(w.keyword_loc().end_offset(), |c| c.location().end_offset());
                let body = w.then_keyword_loc().map_or_else(|| self.term_after(last), |t| t.end_offset());
                self.body.insert(kw, body);
                self.term_headers.insert(kw);
                first = false;
            }
        }
        if let Some(e) = n.else_clause() {
            self.owner.insert(e.else_keyword_loc().start_offset(), kw);
        }
        ruby_prism::visit_case_node(self, n);
    }

    fn visit_case_match_node(&mut self, n: &ruby_prism::CaseMatchNode<'pr>) {
        let kw = n.case_keyword_loc().start_offset();
        self.ctx.push(kw);
        let mut first = true;
        for c in n.conditions().iter() {
            let Some(i) = c.as_in_node() else { continue };
            self.owner.insert(i.in_loc().start_offset(), kw);
            if let Some(t) = i.then_loc() {
                self.owner.insert(t.start_offset(), kw);
            }
            if first {
                self.violation(i.in_loc().start_offset(), 2.7);
                let body = i
                    .then_loc()
                    .map_or_else(|| self.term_after(i.pattern().location().end_offset()), |t| t.end_offset());
                self.body.insert(kw, body);
                self.term_headers.insert(kw);
                first = false;
            }
        }
        if let Some(e) = n.else_clause() {
            self.owner.insert(e.else_keyword_loc().start_offset(), kw);
        }
        ruby_prism::visit_case_match_node(self, n);
        self.ctx.pop();
    }

    fn visit_begin_node(&mut self, n: &ruby_prism::BeginNode<'pr>) {
        if let Some(kw) = n.begin_keyword_loc() {
            let kw = kw.start_offset();
            self.with_owner(kw, n.begin_keyword_loc().map_or(kw, |l| l.end_offset()), |s| {
                s.begin_clauses(n);
                ruby_prism::visit_begin_node(s, n);
            });
        } else {
            ruby_prism::visit_begin_node(self, n);
        }
    }

    fn visit_block_node(&mut self, n: &ruby_prism::BlockNode<'pr>) {
        let open = n.opening_loc().start_offset();
        let body = n
            .parameters()
            .and_then(|p| p.as_block_parameters_node())
            .and_then(|p| p.closing_loc())
            .map_or(n.opening_loc().end_offset(), |c| c.end_offset());
        let body_node = n.body().and_then(|b| b.as_begin_node());
        self.ctx.push(open);
        // `rescue`/`ensure` in a do...end block: 2.5.
        if let Some(b) = &body_node {
            if let Some(r) = b.rescue_clause() {
                self.violation(r.keyword_loc().start_offset(), 2.5);
            } else if let Some(e) = b.ensure_clause() {
                self.violation(e.ensure_keyword_loc().start_offset(), 2.5);
            }
        }
        self.with_owner(open, body, |s| {
            if let Some(b) = &body_node {
                s.begin_clauses(b);
            }
            ruby_prism::visit_block_node(s, n);
        });
        self.ctx.pop();
    }

    fn visit_lambda_node(&mut self, n: &ruby_prism::LambdaNode<'pr>) {
        let open = n.opening_loc().start_offset();
        let body_node = n.body().and_then(|b| b.as_begin_node());
        self.ctx.push(n.location().start_offset());
        // `rescue`/`ensure` in a `-> do ... end` body: 2.6 (`lambda_body`'s do_body).
        if n.opening_loc().as_slice() == b"do"
            && let Some(b) = &body_node
        {
            if let Some(r) = b.rescue_clause() {
                self.violation(r.keyword_loc().start_offset(), 2.6);
            } else if let Some(e) = b.ensure_clause() {
                self.violation(e.ensure_keyword_loc().start_offset(), 2.6);
            }
        }
        self.with_owner(open, n.opening_loc().end_offset(), |s| {
            if let Some(b) = &body_node {
                s.begin_clauses(b);
            }
            ruby_prism::visit_lambda_node(s, n);
        });
        self.ctx.pop();
    }

    fn visit_or_node(&mut self, n: &ruby_prism::OrNode<'pr>) {
        if n.operator_loc().as_slice() == b"or" && command_asgn(&n.left()) {
            self.violation(n.operator_loc().start_offset(), 3.3);
        }
        ruby_prism::visit_or_node(self, n);
    }

    fn visit_and_node(&mut self, n: &ruby_prism::AndNode<'pr>) {
        if n.operator_loc().as_slice() == b"and" && command_asgn(&n.left()) {
            self.violation(n.operator_loc().start_offset(), 3.3);
        }
        ruby_prism::visit_and_node(self, n);
    }

    fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
        // The `do` of a command call's block (`foo a do`) lexes as kDO_BLOCK.
        if n.opening_loc().is_none()
            && n.arguments().is_some()
            && let Some(b) = n.block().and_then(|b| b.as_block_node()).filter(|b| b.opening_loc().as_slice() == b"do")
        {
            self.do_block.insert(b.opening_loc().start_offset());
        }
        // Below 3.3 `command do_block` is a block_command, not a call argument: it cannot be the
        // only argument in `(...)` or `[...]` (`foo(bar baz do end)`; #239).
        if n.opening_loc().is_some()
            && let Some(args) = n.arguments()
            && args.arguments().len() == 1
            && let Some(inner) = args.arguments().iter().next().and_then(|a| a.as_call_node())
            && inner.opening_loc().is_none()
            && inner.arguments().is_some()
            && let Some(b) =
                inner.block().and_then(|b| b.as_block_node()).filter(|b| b.opening_loc().as_slice() == b"do")
        {
            self.violation(b.opening_loc().start_offset(), 3.3);
        }
        // Below 3.3 `not x` without parentheses is an expr, never an assigned value (`x = not y`;
        // #240); as an argument, see visit_arguments_node.
        if self.target < 3.3
            && let Some(m) = n.message_loc().filter(|m| m.as_slice() == b"not")
            && n.opening_loc().is_none()
        {
            let i = self.toks.partition_point(|t| t.start < m.start_offset());
            if i > 0 && matches!(self.toks[i - 1].name, "tEQL" | "tOP_ASGN") {
                let next = self.next_token(m.end_offset());
                self.violation(next, 3.3);
            }
        }
        // A block on an index call (`a[k] { }`, `a[k] do end`): 2.5. Its `do` is a kDO_BLOCK.
        if n.name().as_slice() == b"[]"
            && n.opening_loc().is_some_and(|o| o.as_slice() == b"[")
            && let Some(b) = n.block().and_then(|b| b.as_block_node())
        {
            let open = b.opening_loc().start_offset();
            if self.target < 2.5 {
                if b.opening_loc().as_slice() == b"do" {
                    self.do_block.insert(open);
                } else {
                    self.brace_arg.insert(open);
                }
            }
            self.violation(open, 2.5);
        }
        // `command_args` (the parser pushes cmdarg around them): a call without parentheses
        // that is not an operator or attribute assignment.
        let name = String::from_utf8_lossy(n.name().as_slice());
        if n.opening_loc().is_none()
            && n.message_loc().is_some()
            && !crate::helpers::node_types::is_operator_method(&name)
            && !name.ends_with('=')
        {
            let block_arg = n.block().filter(|b| b.as_block_argument_node().is_some());
            self.command_args(n.arguments(), block_arg);
        }
        let start = n.message_loc().map_or(n.location().start_offset(), |m| m.start_offset());
        self.in_ctx(start, |s| ruby_prism::visit_call_node(s, n));
    }

    fn visit_super_node(&mut self, n: &ruby_prism::SuperNode<'pr>) {
        if n.lparen_loc().is_none() {
            self.command_args(n.arguments(), n.block().filter(|b| b.as_block_argument_node().is_some()));
        }
        self.in_ctx(n.keyword_loc().start_offset(), |s| ruby_prism::visit_super_node(s, n));
    }

    fn visit_yield_node(&mut self, n: &ruby_prism::YieldNode<'pr>) {
        if n.lparen_loc().is_none() {
            self.command_args(n.arguments(), None);
        }
        self.in_ctx(n.keyword_loc().start_offset(), |s| ruby_prism::visit_yield_node(s, n));
    }

    fn visit_string_node(&mut self, n: &ruby_prism::StringNode<'pr>) {
        self.heredoc(n.opening_loc(), Some(n.content_loc().start_offset()), n.closing_loc());
    }

    fn visit_x_string_node(&mut self, n: &ruby_prism::XStringNode<'pr>) {
        self.heredoc(Some(n.opening_loc()), Some(n.content_loc().start_offset()), Some(n.closing_loc()));
    }

    fn visit_interpolated_string_node(&mut self, n: &ruby_prism::InterpolatedStringNode<'pr>) {
        let body = n.parts().iter().next().map(|p| p.location().start_offset());
        self.heredoc(n.opening_loc(), body, n.closing_loc());
        ruby_prism::visit_interpolated_string_node(self, n);
    }

    fn visit_interpolated_x_string_node(&mut self, n: &ruby_prism::InterpolatedXStringNode<'pr>) {
        let body = n.parts().iter().next().map(|p| p.location().start_offset());
        self.heredoc(Some(n.opening_loc()), body, Some(n.closing_loc()));
        ruby_prism::visit_interpolated_x_string_node(self, n);
    }

    fn visit_parentheses_node(&mut self, n: &ruby_prism::ParenthesesNode<'pr>) {
        self.body.insert(n.opening_loc().start_offset(), n.opening_loc().end_offset());
        // `tLPAREN_ARG expr rparen` up to 2.3 (stmt from 2.4): no rescue modifier directly inside.
        if self.token_at(n.opening_loc().start_offset()).is_some_and(|t| t.name == "tLPAREN_ARG")
            && let Some(r) = n.body().and_then(|b| b.as_statements_node()).and_then(|b| {
                let body = b.body();
                (body.len() == 1).then(|| body.iter().next().unwrap()).and_then(|x| x.as_rescue_modifier_node())
            })
        {
            self.violation(r.keyword_loc().start_offset(), 2.4);
        }
        ruby_prism::visit_parentheses_node(self, n);
    }

    fn visit_embedded_statements_node(&mut self, n: &ruby_prism::EmbeddedStatementsNode<'pr>) {
        self.body.insert(n.opening_loc().start_offset(), n.opening_loc().end_offset());
        ruby_prism::visit_embedded_statements_node(self, n);
    }

    fn visit_pre_execution_node(&mut self, n: &ruby_prism::PreExecutionNode<'pr>) {
        self.body.insert(n.opening_loc().start_offset(), n.opening_loc().end_offset());
        ruby_prism::visit_pre_execution_node(self, n);
    }

    fn visit_post_execution_node(&mut self, n: &ruby_prism::PostExecutionNode<'pr>) {
        self.body.insert(n.opening_loc().start_offset(), n.opening_loc().end_offset());
        ruby_prism::visit_post_execution_node(self, n);
    }

    fn visit_arguments_node(&mut self, n: &ruby_prism::ArgumentsNode<'pr>) {
        for a in n.arguments().iter() {
            // Below 3.3 call arguments are `arg`s: `not x` without parentheses is an `expr`, so
            // the grammar fails on the token after `not` (#240).
            if let Some(m) = a
                .as_call_node()
                .filter(|c| c.opening_loc().is_none())
                .and_then(|c| c.message_loc())
                .filter(|m| m.as_slice() == b"not")
            {
                let next = self.next_token(m.end_offset());
                self.violation(next, 3.3);
            }
            if let Some(s) = a.as_splat_node().filter(|s| s.expression().is_none()) {
                let next = self.next_token(s.operator_loc().end_offset());
                self.violation(next, 3.2);
            } else if let Some(f) = a.as_forwarding_arguments_node() {
                let at = f.location().start_offset();
                self.violation(at, 2.7);
                // A trailing comma after `...` is 3.3+ (`c(a, ...,)`).
                let next = self.next_token(f.location().end_offset());
                if self.token_at(next).is_some_and(|t| t.name == "tCOMMA") {
                    self.violation(next, 3.3);
                }
                if let Some(&(kw, _)) = self.defs.iter().rev().find(|d| d.1) {
                    self.forwarding.insert(at, kw);
                }
            }
        }
        ruby_prism::visit_arguments_node(self, n);
    }

    fn visit_array_node(&mut self, n: &ruby_prism::ArrayNode<'pr>) {
        self.in_ctx(n.location().start_offset(), |s| {
            for a in n.elements().iter() {
                if let Some(splat) = a.as_splat_node().filter(|x| x.expression().is_none()) {
                    let next = s.next_token(splat.operator_loc().end_offset());
                    s.violation(next, 3.2);
                }
            }
            ruby_prism::visit_array_node(s, n);
        });
    }

    fn visit_hash_node(&mut self, n: &ruby_prism::HashNode<'pr>) {
        self.in_ctx(n.location().start_offset(), |s| {
            for e in n.elements().iter() {
                s.hash_element(&e);
            }
            ruby_prism::visit_hash_node(s, n);
        });
    }

    fn visit_keyword_hash_node(&mut self, n: &ruby_prism::KeywordHashNode<'pr>) {
        for e in n.elements().iter() {
            self.hash_element(&e);
        }
        ruby_prism::visit_keyword_hash_node(self, n);
    }

    fn visit_block_argument_node(&mut self, n: &ruby_prism::BlockArgumentNode<'pr>) {
        if n.expression().is_none() {
            let next = self.next_token(n.operator_loc().end_offset());
            self.violation(next, 3.1);
        }
        ruby_prism::visit_block_argument_node(self, n);
    }

    fn visit_block_parameter_node(&mut self, n: &ruby_prism::BlockParameterNode<'pr>) {
        if n.name().is_none() {
            let next = self.next_token(n.operator_loc().end_offset());
            self.violation(next, 3.1);
        }
    }

    fn visit_no_keywords_parameter_node(&mut self, n: &ruby_prism::NoKeywordsParameterNode<'pr>) {
        self.violation(n.keyword_loc().start_offset(), 2.7);
    }

    fn visit_match_required_node(&mut self, n: &ruby_prism::MatchRequiredNode<'pr>) {
        self.in_ctx(n.location().start_offset(), |s| {
            s.violation(n.operator_loc().start_offset(), 3.0);
            ruby_prism::visit_match_required_node(s, n);
        });
    }

    fn visit_match_predicate_node(&mut self, n: &ruby_prism::MatchPredicateNode<'pr>) {
        // Below 3.3 the value of `value in pat` is an `arg`: a command whose arguments run up to the
        // `in` (`return x in [1]`, which Prism reads as `(return x) in [1]`) fails there (#231).
        let v = n.value();
        let command = v.as_return_node().is_some_and(|r| r.arguments().is_some())
            || v.as_break_node().is_some_and(|r| r.arguments().is_some())
            || v.as_next_node().is_some_and(|r| r.arguments().is_some())
            || v.as_yield_node().is_some_and(|y| y.arguments().is_some() && y.lparen_loc().is_none())
            || v.as_super_node().is_some_and(|y| y.arguments().is_some() && y.lparen_loc().is_none());
        if command {
            self.violation(n.operator_loc().start_offset(), 3.3);
        }
        self.in_ctx(n.location().start_offset(), |s| {
            s.violation(n.operator_loc().start_offset(), 2.7);
            ruby_prism::visit_match_predicate_node(s, n);
        });
    }

    fn visit_find_pattern_node(&mut self, n: &ruby_prism::FindPatternNode<'pr>) {
        self.violation(n.right().location().start_offset(), 3.0);
        ruby_prism::visit_find_pattern_node(self, n);
    }

    fn visit_pinned_expression_node(&mut self, n: &ruby_prism::PinnedExpressionNode<'pr>) {
        self.violation(n.lparen_loc().start_offset(), 3.1);
        ruby_prism::visit_pinned_expression_node(self, n);
    }

    fn visit_pinned_variable_node(&mut self, n: &ruby_prism::PinnedVariableNode<'pr>) {
        if n.variable().as_local_variable_read_node().is_none() {
            self.violation(n.variable().location().start_offset(), 3.1);
        }
        ruby_prism::visit_pinned_variable_node(self, n);
    }

    fn visit_constant_write_node(&mut self, n: &ruby_prism::ConstantWriteNode<'pr>) {
        self.const_write(&n.name_loc(), &n.operator_loc());
        ruby_prism::visit_constant_write_node(self, n);
    }

    fn visit_constant_or_write_node(&mut self, n: &ruby_prism::ConstantOrWriteNode<'pr>) {
        self.const_write(&n.name_loc(), &n.operator_loc());
        ruby_prism::visit_constant_or_write_node(self, n);
    }

    fn visit_constant_and_write_node(&mut self, n: &ruby_prism::ConstantAndWriteNode<'pr>) {
        self.const_write(&n.name_loc(), &n.operator_loc());
        ruby_prism::visit_constant_and_write_node(self, n);
    }

    fn visit_constant_operator_write_node(&mut self, n: &ruby_prism::ConstantOperatorWriteNode<'pr>) {
        self.const_write(&n.name_loc(), &n.binary_operator_loc());
        ruby_prism::visit_constant_operator_write_node(self, n);
    }

    fn visit_constant_path_write_node(&mut self, n: &ruby_prism::ConstantPathWriteNode<'pr>) {
        self.const_write(&n.target().location(), &n.operator_loc());
        ruby_prism::visit_constant_path_write_node(self, n);
    }

    fn visit_constant_path_or_write_node(&mut self, n: &ruby_prism::ConstantPathOrWriteNode<'pr>) {
        self.const_write(&n.target().location(), &n.operator_loc());
        ruby_prism::visit_constant_path_or_write_node(self, n);
    }

    fn visit_constant_path_and_write_node(&mut self, n: &ruby_prism::ConstantPathAndWriteNode<'pr>) {
        self.const_write(&n.target().location(), &n.operator_loc());
        ruby_prism::visit_constant_path_and_write_node(self, n);
    }

    fn visit_constant_path_operator_write_node(&mut self, n: &ruby_prism::ConstantPathOperatorWriteNode<'pr>) {
        self.const_write(&n.target().location(), &n.binary_operator_loc());
        ruby_prism::visit_constant_path_operator_write_node(self, n);
    }

    fn visit_range_node(&mut self, n: &ruby_prism::RangeNode<'pr>) {
        self.in_ctx(n.location().start_offset(), |s| {
            if n.left().is_none() {
                s.violation(n.operator_loc().start_offset(), 2.7);
            }
            if n.right().is_none() {
                let next = s.next_token(n.operator_loc().end_offset());
                s.violation(next, 2.6);
            }
            ruby_prism::visit_range_node(s, n);
        });
    }
}

impl Walk<'_> {
    /// `body`: where the first part starts (the body's first line), if any.
    fn heredoc(
        &mut self,
        open: Option<ruby_prism::Location>,
        body: Option<usize>,
        close: Option<ruby_prism::Location>,
    ) {
        if let (Some(o), Some(c)) = (open, close)
            && o.as_slice().starts_with(b"<<")
        {
            let start = body.unwrap_or(c.start_offset()).min(c.start_offset());
            self.heredocs.push((o.start_offset(), (start, c.end_offset())));
        }
    }

    fn command_args(&mut self, args: Option<ruby_prism::ArgumentsNode>, block_arg: Option<Node>) {
        let mut range = args.map(|a| (a.location().start_offset(), a.location().end_offset()));
        if let Some(b) = block_arg {
            let (s, e) = (b.location().start_offset(), b.location().end_offset());
            range = Some(range.map_or((s, e), |(rs, re)| (rs.min(s), re.max(e))));
        }
        if let Some((s, e)) = range {
            self.cmd_args.insert(s, e);
        }
    }

    fn const_write(&mut self, target: &ruby_prism::Location, op: &ruby_prism::Location) {
        self.const_writes.insert(op.start_offset(), (target.start_offset(), target.end_offset()));
    }

    fn hash_element(&mut self, e: &Node) {
        if let Some(s) = e.as_assoc_splat_node().filter(|s| s.value().is_none()) {
            let next = self.next_token(s.operator_loc().end_offset());
            self.violation(next, 3.2);
        } else if let Some(a) = e.as_assoc_node().filter(|a| a.value().as_implicit_node().is_some()) {
            let next = self.next_token(a.key().location().end_offset());
            self.violation(next, 3.1);
        }
    }
}

/// A chain of calls or variables (`a`, `a.b`, `A::B`, `a&.b(1)[2]`): what a multiple-assignment
/// item can be (only its tokens outside brackets are given).
/// `command_asgn`: an assignment whose value is a command call without parentheses (`a = foo 1`,
/// `a.b += foo 1`, `a = b = foo 1`). Below 3.3 it is a statement, so it cannot be the left operand
/// of `or`/`and`.
fn command_asgn(node: &Node) -> bool {
    let value = match crate::ast::assignment_parts(node) {
        Some((_, v)) if node.as_multi_write_node().is_none() => v,
        _ => {
            let setter = node.as_call_node().filter(|c| c.equal_loc().is_some());
            match setter.and_then(|c| c.arguments()).and_then(|a| a.arguments().iter().last()) {
                Some(v) => v,
                None => return false,
            }
        }
    };
    if command_asgn(&value) {
        return true;
    }
    if let Some(c) = value.as_call_node() {
        let name = String::from_utf8_lossy(c.name().as_slice());
        return c.opening_loc().is_none()
            && c.message_loc().is_some()
            && c.arguments().is_some()
            && !crate::helpers::node_types::is_operator_method(&name)
            && !name.ends_with('=');
    }
    value.as_super_node().is_some_and(|s| s.lparen_loc().is_none() && s.arguments().is_some())
        || value.as_yield_node().is_some_and(|y| y.lparen_loc().is_none() && y.arguments().is_some())
}

fn is_chain(item: &[Tok]) -> bool {
    let operand = |n: &str| matches!(n, "tIDENTIFIER" | "tFID" | "tCONSTANT" | "tIVAR" | "tGVAR" | "tCVAR" | "kSELF");
    // A literal receiver (`2.days.ago`) is a primary_value, but not an item on its own.
    let literal = |n: &str| {
        matches!(n, "tINTEGER" | "tFLOAT" | "tRATIONAL" | "tIMAGINARY" | "tSTRING" | "tSYMBOL" | "tCHARACTER")
    };
    let mut want_operand = true;
    let mut bare_literal = false;
    for (k, t) in item.iter().enumerate() {
        match (want_operand, t.name) {
            (true, n) if operand(n) => want_operand = false,
            (true, n) if k == 0 && literal(n) => (want_operand, bare_literal) = (false, true),
            (true, "tCOLON3") => {}
            (false, "tDOT" | "tANDDOT" | "tCOLON2") => (want_operand, bare_literal) = (true, false),
            (false, "tLPAREN2" | "tRPAREN" | "tLBRACK2" | "tRBRACK") => {}
            _ => return false,
        }
    }
    !want_operand && !bare_literal
}

/// The builder's error for a multiple-assignment item, if any.
fn mlhs_item_error(item: &[Tok], in_def: bool) -> Option<(usize, usize, &'static str)> {
    if let [t] = item
        && matches!(t.name, "kTRUE" | "kFALSE" | "kNIL" | "kSELF" | "k__FILE__" | "k__LINE__" | "k__ENCODING__")
    {
        return Some((t.start, t.end, "cannot assign to a keyword"));
    }
    // A constant target (`A`, `x::A`) while the parser believes it is in a def.
    if in_def
        && let [first, .., last] | [first @ last] = item
        && last.name == "tCONSTANT"
        && (item.len() == 1 || matches!(item[item.len() - 2].name, "tCOLON2" | "tCOLON3"))
        && is_chain(item)
    {
        return Some((first.start, last.end, "dynamic constant assignment"));
    }
    // The item's own call operator (`a&.b.c` is fine); a call with arguments is no item at all.
    if call_with_args(item) {
        return None;
    }
    let anddot = item
        .iter()
        .rev()
        .find(|t| matches!(t.name, "tDOT" | "tANDDOT" | "tCOLON2"))
        .filter(|t| t.name == "tANDDOT" && is_chain(item))?;
    Some((anddot.start, anddot.end, "&. inside multiple assignment destination"))
}

/// `a.b(1)`: a chain ending with parenthesized arguments, which cannot be assigned to.
fn call_with_args(item: &[Tok]) -> bool {
    item.last().is_some_and(|t| t.name == "tRPAREN") && is_chain(item)
}

struct Frame {
    name: &'static str,
    key: usize,
    closer: &'static str,
    body: Option<usize>,
    /// in_def when the construct opened (restored when a def or `class <<` closes).
    in_def: bool,
}

/// The marker `diagnostics` returns when racc gives up at `offset` (nothing is lexed after it):
/// an empty message, which callers drop.
fn gave_up(offset: usize) -> (usize, usize, String) {
    (offset, offset, String::new())
}

/// Runs racc's error recovery over the token stream (see the module doc).
fn simulate(
    toks: &[Tok],
    w: &Walk,
    text: &str,
    target: f64,
    seed: usize,
    actions: &[usize],
    circular: &[(usize, usize)],
) -> Vec<(usize, usize, String)> {
    const MAX_REPARSES: usize = 256;
    let mut out = Vec::new();
    // A circular argument reference's yyerror leaves the parser's current argument set: later
    // reads of that name report it again (until another scope starts).
    let mut circular_arg: Option<&str> = None;
    // A builder error found here (circular reference, `return` in a class body) yyerrors when its
    // node reduces, with the next token as lookahead.
    let mut pending_yy: Option<usize> = None;
    let mut stack = vec![Frame { name: "", key: usize::MAX, closer: "$end", body: Some(0), in_def: false }];
    // racc's @racc_error_status: errors are reported only at 0; 3 = discarding tokens.
    let mut status = 0u8;
    let mut discarding = false;
    // Where parsing last resumed after an error, and where that parse fails next.
    let mut resume = 0;
    let mut fails_at = seed;
    // `return`s the current resumed parse finds directly in a class body.
    let mut bad_returns: Vec<usize> = Vec::new();
    let mut reparses = 0;
    let mut first_failure = |stack: &[Frame], from: usize| {
        reparses += 1;
        if reparses > MAX_REPARSES {
            return (usize::MAX, Vec::new());
        }
        // Each open construct's header, made to parse on its own.
        let prefix: String = stack[1..]
            .iter()
            .map(|f| {
                let header = match (f.body, f.name) {
                    (Some(b), _) => text.get(f.key..b).unwrap_or(""),
                    // Brackets left open around a construct recovery stays in (`foo(bar do`).
                    (None, "tLPAREN2" | "tLPAREN_ARG") => "_(",
                    (None, "tLBRACK2") => "_[",
                    (None, "tLBRACK") => "[",
                    (None, "tLBRACE") => "{_:",
                    _ => "",
                };
                let lead = match f.name {
                    "kDO" | "tLCURLY" => "_ ",
                    "tLAMBEG" | "kDO_LAMBDA" => "-> ",
                    "tSTRING_DBEG" => "\"",
                    _ => "",
                };
                format!("{lead}{header}\n")
            })
            .collect();
        // Clause keywords of popped constructs are judged by the simulation.
        let blank: Vec<(usize, usize)> =
            w.heredocs.iter().filter(|h| h.0 < from && h.1.0 >= from).map(|h| h.1).collect();
        let (first, returns) = first_error_after(text, &prefix, from, target, |o| w.owner.contains_key(&o), &blank);
        (first.unwrap_or(usize::MAX), returns)
    };
    // A failure offset fails at the first token at or after it in source order, reached in lexer
    // order (a heredoc's body is lexed before the rest of its opening line).
    let mut by_offset: Vec<(usize, usize)> = toks.iter().enumerate().map(|(i, t)| (t.start, i)).collect();
    by_offset.sort_unstable();
    let fail_index = |at: usize| by_offset.get(by_offset.partition_point(|&(s, _)| s < at)).map_or(usize::MAX, |p| p.1);
    // A semantic action's yyerror (racc's C core) cuts the reduced rule's states but searches for
    // an `error` shift from the stale current state, so it first pops one more: the state the
    // construct starting at `key` was parsed in. When that is the first state of the enclosing
    // body, the enclosing construct is lost too.
    let at_body_start = |stack: &[Frame], key: usize| {
        let top = stack.last().unwrap();
        let i = fail_index(key);
        stack.len() > 1
            && i > 0
            && i != usize::MAX
            && w.term_headers.contains(&top.key)
            && top.body == Some(toks[i - 1].end)
    };
    // The last token shifted.
    let mut prev = "";
    let mut prev_end = 0;
    // The parser's @context.in_def.
    let mut in_def = false;
    let mut next_action = 0;
    // The current statement's frame depth, whether it can still be a multiple assignment, and the
    // tokens of its current item at that depth.
    let (mut stmt_depth, mut mlhs_ok, mut item) = (1, true, Vec::<Tok>::new());
    // The parser gem's cmdarg stack (top last): its lexer pushes false at each bracket and pops at
    // its closer; the parser pushes true around command arguments and false around block, def,
    // class, ... bodies, and pops when they reduce. Recovery skips the pops of what it discards,
    // so a `do` after an error inside command arguments lexes as kDO_BLOCK.
    let mut cmdarg = vec![false];
    // Open command arguments: (end offset, frame depth of the call's statement).
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let opens_bracket = |n: &str| {
        matches!(n, "tLPAREN" | "tLPAREN2" | "tLPAREN_ARG" | "tLBRACK" | "tLBRACK2" | "tLBRACE" | "tLCURLY" | "tLAMBEG")
    };
    let action_frame = |n: &str| {
        matches!(
            n,
            "kDO"
                | "tLCURLY"
                | "kDEF"
                | "kCLASS"
                | "kSCLASS"
                | "kMODULE"
                | "kBEGIN"
                | "tLAMBEG"
                | "kDO_LAMBDA"
                | "tSTRING_DBEG"
        )
    };
    // The parser's @context.in_kwarg: set while a def's unparenthesized parameter list parses, and
    // left set when an error abandons it (a label ending its line then lexes a tNL).
    let mut in_kwarg = false;
    // Index of the last `,` that continued a multiple-assignment target list.
    let mut mlhs_comma: Option<usize> = None;
    let mut dangling_then = false;
    // Constructs whose `rescue` recovery discarded, and those with a useless `else` (2.6+).
    let mut lost_rescue: HashSet<usize> = HashSet::new();
    let mut useless_else: HashSet<usize> = HashSet::new();
    for (i, t) in toks.iter().enumerate() {
        if t.name == "tLABEL_NL" && !in_kwarg {
            continue;
        }
        let kdo_block = t.name == "kDO" && !t.cmd_do && !w.do_block.contains(&t.start) && cmdarg.last() == Some(&true);
        if opens_bracket(t.name) {
            cmdarg.push(false);
        } else if matches!(t.name, "tRPAREN" | "tRBRACK" | "tRCURLY") {
            cmdarg.pop();
        }
        // Command arguments that ended before `t` reduce with `t` as lookahead (below a bracket
        // `t` itself pushed).
        while regions.last().is_some_and(|r| r.0 <= t.start) {
            regions.pop();
            let top = if opens_bracket(t.name) { cmdarg.pop() } else { None };
            cmdarg.pop();
            cmdarg.extend(top);
        }
        let mut resumed = false;
        if discarding {
            if !starts_stmt(t.name) {
                if t.name == "$end" {
                    break;
                }
                // A discarded clause keyword leaves its `then` without a clause.
                if matches!(t.name, "kWHEN" | "kELSIF" | "kIN" | "kRESCUE") {
                    dangling_then = true;
                }
                if t.name == "kRESCUE"
                    && let Some(&o) = w.owner.get(&t.start)
                {
                    lost_rescue.insert(o);
                }
                continue;
            }
            discarding = false;
            resumed = true;
            resume = t.start;
            (fails_at, bad_returns) = first_failure(&stack, resume);
            (stmt_depth, mlhs_ok) = (stack.len(), true);
            item.clear();
        }
        if matches!(t.name, "kDEF" | "kCLASS" | "kMODULE") {
            circular_arg = None;
        }
        let name = &text[t.start..t.end];
        let read = t.name == "tIDENTIFIER"
            && !matches!(prev, "tDOT" | "tANDDOT" | "tCOLON2" | "kDEF")
            && !toks.get(i + 1).is_some_and(|n| matches!(n.name, "tEQL" | "tOP_ASGN" | "tLPAREN2"));
        // (A default value recovery resumes at is a statement of its own, its parameter undeclared.)
        if (!resumed && circular.iter().any(|c| c.0 == t.start)) || (read && circular_arg == Some(name)) {
            out.push((t.start, t.end, format!("circular argument reference {name}")));
            circular_arg = Some(name);
            pending_yy = Some(t.end);
        }
        if t.name == "kRETURN" && !in_def && bad_returns.contains(&t.start) {
            out.push((t.start, t.end, "Invalid return in class/module body".to_string()));
            pending_yy = Some(t.end);
        }
        if let Some(&end) = w.cmd_args.get(&t.start) {
            let top = if opens_bracket(t.name) { cmdarg.pop() } else { None };
            cmdarg.push(true);
            cmdarg.extend(top);
            regions.push((end, stack.len()));
        }
        // Multiple-assignment items (`a, b = ...`, which a resumed `x, y` parses as): the builder
        // checks each one as its `,` arrives (before any grammar failure further on); a keyword or
        // a `&.` call is an error (and yyerror).
        if t.name == "tCOMMA"
            && stack.len() == stmt_depth
            && mlhs_ok
            && let Some((s, e, msg)) = mlhs_item_error(&item, in_def)
        {
            out.push((s, e, msg.to_string()));
            regions.retain(|r| r.1 < stack.len());
            (status, discarding, fails_at, mlhs_ok) = (3, true, usize::MAX, false);
            continue;
        }
        let top = stack.last().unwrap();
        // `rescue`/`else`/`ensure` left over from a popped construct still fit a body that takes
        // them (`else` without `rescue` is an error of its own from 2.6).
        let clause_fits = matches!(t.name, "kRESCUE" | "kELSE" | "kENSURE")
            && (matches!(top.name, "kDEF" | "kCLASS" | "kMODULE" | "kSCLASS" | "kBEGIN")
                || (top.name == "kDO" && target >= 2.5));
        let misplaced = w.owner.get(&t.start).is_some_and(|&o| o != top.key);
        // ...as does an `else` whose `rescue` recovery discarded.
        let lost = t.name == "kELSE" && !misplaced && lost_rescue.contains(&top.key);
        if ((misplaced && clause_fits) || lost) && t.name == "kELSE" && target >= 2.6 {
            out.push((t.start, t.end, "else without rescue is useless".to_string()));
            if lost {
                useless_else.insert(top.key);
            }
        }
        let error = (!resumed && w.violations.get(&t.start).is_some_and(|&ctx| ctx >= resume))
            || i >= fail_index(fails_at)
            || (is_closer(t.name) && t.name != top.closer)
            || (misplaced && !clause_fits)
            || w.forwarding.get(&t.start).is_some_and(|kw| !stack.iter().any(|f| f.key == *kw))
            || (t.name == "$end" && (stack.len() > 1 || wants_operand(prev)))
            || kdo_block
            || t.name == "tLABEL_NL"
            || (t.name == "kTHEN" && dangling_then)
            // A label cannot start a multiple-assignment item.
            || (t.name == "tLABEL" && mlhs_comma.is_some_and(|c| c + 1 == i))
            || (t.name == "tCOMMA" && stack.len() == stmt_depth && mlhs_ok && call_with_args(&item));
        // A semantic error's yyerror fires while reducing, with the next token as lookahead.
        let mut yyerror = false;
        while next_action < actions.len() && actions[next_action] <= t.start {
            next_action += 1;
            yyerror = true;
        }
        if pending_yy.is_some_and(|o| o <= t.start) {
            pending_yy = None;
            yyerror = true;
        }
        if error || yyerror {
            if t.name == "kTHEN" {
                dangling_then = false;
            }
            if status == 0 && error {
                let name = match t.name {
                    _ if w.do_block.contains(&t.start) || kdo_block => "kDO_BLOCK",
                    _ if w.brace_arg.contains(&t.start) => "tLBRACE_ARG",
                    "tLABEL_NL" => "tNL",
                    n => n,
                };
                out.push((t.start, t.end, format!("unexpected token {name}")));
            }
            let top = stack.last().unwrap();
            if error
                && top.name == "kDEF"
                && prev != "kDEF"
                && w.term_headers.contains(&top.key)
                && top.body.is_none_or(|b| t.start < b)
            {
                in_kwarg = true;
            }
            // After a complete statement LALR default reductions have already reduced the enclosing
            // body: from 2.7 at the top level that is the whole program and racc gives up; from 2.4
            // a block's body reduces with its parameters, leaving no state to shift `error` in it.
            let complete = prev.is_empty() || matches!(prev, "tNL" | "tSEMI") || ends_expr(prev);
            if target >= 2.7 && stack.len() == 1 && complete && error {
                out.push(gave_up(t.start));
                break;
            }
            status = 3;
            let unbegun = |f: &Frame| f.body.is_none_or(|b| t.start < b);
            // The statement before the error is the body's own (no construct opened since).
            let direct = !unbegun(stack.last().unwrap());
            // Right at the start of the body, it reduces as empty.
            let empty_body = stack.last().unwrap().body == Some(prev_end);
            while stack.len() > 1 && unbegun(stack.last().unwrap()) {
                stack.pop();
            }
            if (complete || empty_body)
                && direct
                && target >= 2.4
                && matches!(stack.last().unwrap().name, "kDO" | "tLCURLY")
            {
                // The block body's reduction runs its action, which pops cmdarg.
                stack.pop();
                cmdarg.pop();
                while stack.len() > 1 && unbegun(stack.last().unwrap()) {
                    stack.pop();
                }
            }
            regions.retain(|r| r.1 < stack.len());
            // The failing token is retried in the `error . stmt` state.
            if t.name == "$end" {
                break;
            }
            fails_at = usize::MAX;
            if !starts_stmt(t.name) {
                discarding = true;
                continue;
            }
            resume = t.start;
            (fails_at, bad_returns) = first_failure(&stack, resume);
            (stmt_depth, mlhs_ok) = (stack.len(), true);
            item.clear();
        }
        if t.name == "tCOMMA" && stack.len() == stmt_depth && mlhs_ok {
            mlhs_ok = is_chain(&item);
            if mlhs_ok {
                mlhs_comma = Some(i);
            }
            item.clear();
        } else if stack.len() == stmt_depth || (is_closer(t.name) && stack.len() == stmt_depth + 1) {
            item.push(*t);
        }
        // Assigning a constant while the parser believes it is in a def (see in_def): the
        // builder's error, then recovery from the `=`.
        if let Some(&(s, e)) = w.const_writes.get(&t.start).filter(|_| in_def) {
            out.push((s, e, "dynamic constant assignment".to_string()));
            while stack.len() > 1 && stack.last().unwrap().body.is_none_or(|b| t.start < b) {
                stack.pop();
            }
            if at_body_start(&stack, s) {
                stack.pop();
            }
            regions.retain(|r| r.1 < stack.len());
            (status, discarding, fails_at) = (3, true, usize::MAX);
            continue;
        }
        // The body with a useless `else` reduces at its `end`: the builder's yyerror, and racc
        // recovers inside the construct, retrying (and so discarding) the `end`.
        if t.name == "kEND" && useless_else.remove(&stack.last().unwrap().key) {
            regions.retain(|r| r.1 < stack.len());
            (status, discarding, fails_at) = (3, true, usize::MAX);
            continue;
        }
        if is_closer(t.name) {
            if stack.len() > 1 {
                let f = stack.pop().unwrap();
                if action_frame(f.name) && !(matches!(f.name, "kCLASS" | "kMODULE") && in_def) {
                    cmdarg.pop();
                }
                match f.name {
                    "kDEF" | "kSCLASS" => in_def = f.in_def,
                    // A def popped by recovery never restores the parser's in_def flag. The
                    // action's error (yyerror) starts recovery without an `unexpected token`;
                    // from 2.7 racc gives up when the construct is the file's first statement.
                    "kCLASS" | "kMODULE" if in_def => {
                        let (len, what) = if f.name == "kCLASS" { (5, "class") } else { (6, "module") };
                        out.push((f.key, f.key + len, format!("{what} definition in method body")));
                        let first = toks.iter().find(|t| t.name != "tNL").is_some_and(|t| t.start == f.key);
                        if target >= 2.7 && stack.len() == 1 && first {
                            out.push(gave_up(t.start));
                            break;
                        }
                        if at_body_start(&stack, f.key) {
                            stack.pop();
                        }
                        regions.retain(|r| r.1 < stack.len());
                        (status, discarding, fails_at, prev) = (3, true, usize::MAX, t.name);
                        continue;
                    }
                    _ => {}
                }
            }
        } else if let Some(closer) = closer_of(t.name).filter(|_| !(t.name == "kDEF" && w.endless.contains(&t.start))) {
            let sclass = t.name == "kCLASS" && toks.get(i + 1).is_some_and(|n| n.name == "tLSHFT");
            let name = if sclass { "kSCLASS" } else { t.name };
            stack.push(Frame { name, key: t.start, closer, body: w.body.get(&t.start).copied(), in_def });
            if action_frame(name) {
                cmdarg.push(false);
            }
            match name {
                "kDEF" => in_def = true,
                "kSCLASS" => in_def = false,
                _ => {}
            }
        }
        if t.name == "$end" {
            break;
        }
        status = status.saturating_sub(1);
        prev = t.name;
        prev_end = t.end;
        if matches!(t.name, "tNL" | "tSEMI" | "kTHEN") {
            dangling_then = false;
        }
        if matches!(t.name, "tNL" | "tSEMI") {
            (stmt_depth, mlhs_ok) = (stack.len(), true);
            item.clear();
        }
    }
    out
}
