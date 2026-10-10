//! Prism lexer tokens shaped like rubocop-ast's `processed_source.sorted_tokens` (parser gem tokens).

use crate::source::{Source, Token};
use ruby_prism_sys as sys;

/// Prism tokens minus those the parser gem has no counterpart for: newlines the grammar ignores
/// (parser emits `tNL` only for significant ones) and the body/end of `=begin` blocks (parser
/// emits one `tCOMMENT` for the whole block; `EMBDOC_BEGIN` stands in for it).
/// Not modelled: parser's `tNL` after every comment and zero-width `tSPACE` closing `%w()`;
/// Prism's `COMMENT` includes the newline (parser's comment ends before it).
pub fn sorted_tokens(src: &Source) -> Vec<Token> {
    src.tokens()
        .iter()
        .filter(|t| {
            !matches!(t.kind, sys::PM_TOKEN_IGNORED_NEWLINE | sys::PM_TOKEN_EMBDOC_LINE | sys::PM_TOKEN_EMBDOC_END)
        })
        .copied()
        .collect()
}

/// Token#line equality (both tokens' begin lines).
pub fn same_line(src: &Source, a: &Token, b: &Token) -> bool {
    src.line_of(a.start) == src.line_of(b.start)
}

/// Token#left_curly_brace? (tLCURLY / tLAMBEG). Prism uses BRACE_LEFT for hash braces too.
pub fn left_curly_brace(t: &Token) -> bool {
    matches!(t.kind, sys::PM_TOKEN_BRACE_LEFT | sys::PM_TOKEN_LAMBDA_BEGIN)
}
