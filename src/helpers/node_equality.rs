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
    tokens[first..]
        .iter()
        .take_while(|t| t.end <= end)
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
    src.text[a.0..a.1] == src.text[b.0..b.1] || shape(src, a.0, a.1) == shape(src, b.0, b.1)
}
