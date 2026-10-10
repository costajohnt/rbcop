//! RuboCop's SpaceAfterPunctuation mixin (SpaceAfterComma, SpaceAfterSemicolon).

use crate::cop::Cx;
use crate::helpers::parser_tokens::{same_line, sorted_tokens};
use crate::source::Token;
use ruby_prism_sys as sys;

/// Reports `Space missing after <kind>.` on each token for which `kind(token, next)` names a kind,
/// the next token starts right after it on the same line, and the next token needs a space before
/// it. `rcurly_style` is the EnforcedStyle that decides whether `}` may follow without a space.
pub fn check(cx: &mut Cx, rcurly_style: &str, kind: impl Fn(&Token, &Token) -> Option<&'static str>) {
    let src = cx.src;
    // ponytail: RuboCop walks unsorted parser tokens here; sorted order only differs around
    // heredoc bodies, where the neighbour is on another line either way.
    for pair in sorted_tokens(src).windows(2) {
        let (t1, t2) = (&pair[0], &pair[1]);
        let Some(k) = kind(t1, t2) else { continue };
        if !same_line(src, t1, t2) || t2.start != t1.end {
            continue;
        }
        let allowed = matches!(
            t2.kind,
            sys::PM_TOKEN_PARENTHESIS_RIGHT | sys::PM_TOKEN_BRACKET_RIGHT | sys::PM_TOKEN_PIPE | sys::PM_TOKEN_EMBEXPR_END
        ) || (t2.kind == sys::PM_TOKEN_BRACE_RIGHT && rcurly_style == "no_space");
        if !allowed {
            cx.add(t1.start, t1.end, format!("Space missing after {k}."));
        }
    }
}

/// `config.for_cop(cop)['EnforcedStyle'] || 'space'`.
pub fn style_of(cx: &Cx, cop: &str) -> String {
    cx.config.for_cop(cop).str("EnforcedStyle").unwrap_or("space").to_string()
}
