//! Capture groups of a regexp literal, as RuboCop's `RegexpNode#each_capture` finds them through
//! regexp_parser on the literal's source (interpolations blanked).

use crate::prelude::*;

/// (named, numbered) capture group counts of a Prism regexp literal; None when parser would call
/// it `interpolation?` (a `#{}` part; `#@ivar` parts stay in the source as written).
pub fn regexp_captures(node: &Node) -> Option<(usize, usize)> {
    if let Some(r) = node.as_regular_expression_node() {
        return Some(count_captures(text(&r.content_loc()).as_bytes(), r.is_extended()));
    }
    let r = node.as_interpolated_regular_expression_node()?;
    if r.parts().iter().any(|p| p.as_embedded_statements_node().is_some()) {
        return None;
    }
    let (open, close) = (r.opening_loc().end_offset(), r.closing_loc().start_offset());
    let src = r.location();
    let base = src.start_offset();
    let all = src.as_slice();
    Some(count_captures(&all[open - base..close - base], r.is_extended()))
}

/// Count `(...)` capture groups in regexp source: plain groups are numbered, `(?<n>` / `(?'n'`
/// named; escapes, character classes, `(?#...)` and (with /x) `#` comments are skipped.
pub fn count_captures(s: &[u8], extended: bool) -> (usize, usize) {
    let (named, numbered) = scan(s, extended);
    (named.len(), numbered)
}

/// `regexp.to_regexp.named_captures.keys` of a non-interpolated regexp literal: every group name,
/// first occurrence order, whether or not it is a valid local variable name (`(?<OS>..)`).
pub fn capture_names(node: &Node) -> Vec<String> {
    let Some(r) = node.as_regular_expression_node() else { return Vec::new() };
    let mut names: Vec<String> = Vec::new();
    for name in scan(r.content_loc().as_slice(), r.is_extended()).0 {
        let name = String::from_utf8_lossy(name).into_owned();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// (named group names, numbered group count).
fn scan(s: &[u8], extended: bool) -> (Vec<&[u8]>, usize) {
    let (mut named, mut numbered) = (Vec::new(), 0);
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 2,
            b'[' => i = skip_class(s, i),
            b'#' if extended => {
                while i < s.len() && s[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' => {
                if s.get(i + 1) != Some(&b'?') {
                    numbered += 1;
                } else {
                    match (s.get(i + 2), s.get(i + 3)) {
                        (Some(b'<'), Some(c)) if *c != b'=' && *c != b'!' => named.push(group_name(s, i + 3, b'>')),
                        (Some(b'\''), _) => named.push(group_name(s, i + 3, b'\'')),
                        (Some(b'#'), _) => {
                            while i < s.len() && s[i] != b')' {
                                i += 1;
                            }
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    (named, numbered)
}

/// The group name starting at `s[from]`, up to `close`.
fn group_name(s: &[u8], from: usize, close: u8) -> &[u8] {
    let rest = &s[from.min(s.len())..];
    &rest[..rest.iter().position(|&b| b == close).unwrap_or(rest.len())]
}

/// Index just past the character class starting at `s[i] == '['` (nested classes included).
fn skip_class(s: &[u8], mut i: usize) -> usize {
    let mut depth = 0;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::count_captures;

    #[test]
    fn captures() {
        assert_eq!(count_captures(br"(?<a>x)(y)\((?:z)[(]", false), (1, 1));
        assert_eq!(count_captures(b"(?'a'x)(?<=y)(?<!z)(?#(c)(d)", false), (1, 1));
        assert_eq!(count_captures(b"a # (x)\n(y)", true), (0, 1));
        assert_eq!(count_captures(b"[[:alpha:](]()", false), (0, 1));
    }
}
