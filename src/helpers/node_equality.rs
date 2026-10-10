//! Stand-in for rubocop-ast structural node equality (`node1 == node2`, `Set#add?`, `include?`).

use crate::source::Source;
use ruby_prism_sys as sys;

/// The significant token texts of a byte range: comments and ignorable newlines dropped,
/// statement separators (`;` / newline) unified. Two nodes with the same shape are equal.
/// Plain `'..'` / `".."` literals without escapes or interpolation compare by value (`'a'` equals
/// `"a"`, both parser `(str "a")`).
/// ponytail: token-based, so `foo()` vs `foo` or escaped vs unescaped spellings of one string count
/// as different although parser builds the same node; walk the Prism tree if that ever matters.
pub fn shape(src: &Source, start: usize, end: usize) -> Vec<String> {
    let tokens = src.tokens();
    let first = tokens.partition_point(|t| t.start < start);
    let in_range: Vec<&crate::source::Token> = tokens[first..].iter().take_while(|t| t.end <= end).collect();
    let mut out = significant(src, in_range.iter().copied());
    // A heredoc's body lies after its opening line, outside the node's range, but parser's node
    // includes it: append each body, matched to its opener by order on that line.
    for opener in in_range.iter().filter(|t| t.kind == sys::PM_TOKEN_HEREDOC_START) {
        let line = src.line_of(opener.start);
        let line_from = tokens.partition_point(|t| t.start < src.line_start(line));
        let nth = tokens[line_from..opener_index(tokens, opener)]
            .iter()
            .filter(|t| t.kind == sys::PM_TOKEN_HEREDOC_START && src.line_of(t.start) == line)
            .count();
        if line < src.line_count() {
            let body_from = tokens.partition_point(|t| t.start < src.line_start(line + 1));
            let body = tokens[body_from..].split_inclusive(|t| t.kind == sys::PM_TOKEN_HEREDOC_END).nth(nth);
            out.push("<<".to_string());
            out.extend(significant(src, body.unwrap_or_default().iter()));
        }
    }
    out
}

fn opener_index(tokens: &[crate::source::Token], opener: &crate::source::Token) -> usize {
    tokens.partition_point(|t| (t.start, t.end) < (opener.start, opener.end))
}

fn significant<'a>(src: &Source, tokens: impl Iterator<Item = &'a crate::source::Token>) -> Vec<String> {
    let text = |t: &crate::source::Token| &src.text[t.start..t.end];
    let mut out: Vec<String> = Vec::new();
    // A plain quoted string seen so far: (index in `out` of its opening quote, quote).
    let mut open: Option<(usize, &str)> = None;
    for t in tokens {
        if matches!(
            t.kind,
            sys::PM_TOKEN_COMMENT
                | sys::PM_TOKEN_IGNORED_NEWLINE
                | sys::PM_TOKEN_EMBDOC_BEGIN
                | sys::PM_TOKEN_EMBDOC_LINE
                | sys::PM_TOKEN_EMBDOC_END
                | sys::PM_TOKEN_EOF
        ) {
            continue;
        }
        let s = text(t);
        match (t.kind, open) {
            (sys::PM_TOKEN_STRING_END, Some((i, q))) if s == q && out.len() <= i + 2 => {
                let value = out.get(i + 1).cloned().unwrap_or_default();
                out.truncate(i);
                out.push(format!("\0str:{value}"));
                open = None;
                continue;
            }
            (sys::PM_TOKEN_STRING_CONTENT, Some((i, _))) if out.len() == i + 1 && !s.contains('\\') => {}
            _ => open = None,
        }
        if t.kind == sys::PM_TOKEN_STRING_BEGIN && (s == "'" || s == "\"") {
            open = Some((out.len(), s));
        }
        out.push(match t.kind {
            sys::PM_TOKEN_NEWLINE | sys::PM_TOKEN_SEMICOLON => ";".to_string(),
            _ => s.to_string(),
        });
    }
    out
}

/// `node1 == node2` for two byte ranges.
pub fn same(src: &Source, a: (usize, usize), b: (usize, usize)) -> bool {
    let (ta, tb) = (&src.text[a.0..a.1], &src.text[b.0..b.1]);
    // Equal text proves equality unless a heredoc opener hides different bodies.
    (ta == tb && !ta.contains("<<")) || shape(src, a.0, a.1) == shape(src, b.0, b.1)
}
