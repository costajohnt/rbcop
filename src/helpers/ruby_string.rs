//! Ruby String semantics that differ from Rust's (Unicode-aware) defaults.

/// String#strip: removes ASCII whitespace and NUL only.
pub fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\0'))
}
