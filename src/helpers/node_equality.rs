//! Stand-in for rubocop-ast structural node equality (`node1 == node2`, `Set#add?`, `include?`).

use crate::source::Source;
use ruby_prism_sys as sys;

/// The significant token texts of a byte range: comments and ignorable newlines dropped,
/// statement separators (`;` / newline) unified. Two nodes with the same shape are equal.
/// ponytail: token-based, so `'a'` vs `"a"` or `foo()` vs `foo` count as different although parser
/// builds the same node; walk the Prism tree if that ever matters.
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
    tokens
        .filter(|t| {
            !matches!(
                t.kind,
                sys::PM_TOKEN_COMMENT
                    | sys::PM_TOKEN_IGNORED_NEWLINE
                    | sys::PM_TOKEN_EMBDOC_BEGIN
                    | sys::PM_TOKEN_EMBDOC_LINE
                    | sys::PM_TOKEN_EMBDOC_END
                    | sys::PM_TOKEN_EOF
            )
        })
        .map(|t| match t.kind {
            sys::PM_TOKEN_NEWLINE | sys::PM_TOKEN_SEMICOLON => ";".to_string(),
            _ => src.text[t.start..t.end].to_string(),
        })
        .collect()
}

/// `node1 == node2` for two byte ranges.
pub fn same(src: &Source, a: (usize, usize), b: (usize, usize)) -> bool {
    let (ta, tb) = (&src.text[a.0..a.1], &src.text[b.0..b.1]);
    // Equal text proves equality unless a heredoc opener hides different bodies.
    (ta == tb && !ta.contains("<<")) || shape(src, a.0, a.1) == shape(src, b.0, b.1)
}
