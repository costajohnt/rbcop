//! RuboCop's SpaceBeforePunctuation mixin (SpaceBeforeComma, SpaceBeforeSemicolon).

use crate::cop::Cx;
use crate::helpers::parser_tokens::{left_curly_brace, same_line, sorted_tokens};
use crate::helpers::space_after_punctuation::style_of;

/// Reports `Space found before <kind>.` on the gap between a token of kind `kind` (e.g. COMMA)
/// and the token before it on the same line, unless that is a `{` that wants a space after it.
pub fn check(cx: &mut Cx, kind: u32, name: &str) {
    let src = cx.src;
    let lcurly_needs_space = style_of(cx, "Layout/SpaceInsideBlockBraces") == "space";
    for pair in sorted_tokens(src).windows(2) {
        let (t1, t2) = (&pair[0], &pair[1]);
        if t2.kind != kind || !same_line(src, t1, t2) || t2.start <= t1.end {
            continue;
        }
        if left_curly_brace(t1) && lcurly_needs_space {
            continue;
        }
        // PunctuationCorrector.remove_space
        cx.add_fix(t1.end, t2.start, format!("Space found before {name}."), |c| c.remove(t1.end, t2.start));
    }
}
