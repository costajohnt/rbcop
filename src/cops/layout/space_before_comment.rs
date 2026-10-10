use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceBeforeComment";
pub static COP: SpaceBeforeComment = SpaceBeforeComment;
pub struct SpaceBeforeComment;

impl Cop for SpaceBeforeComment {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        for pair in helpers::parser_tokens::sorted_tokens(src).windows(2) {
            let (t1, t2) = (&pair[0], &pair[1]);
            if !matches!(t2.kind, sys::PM_TOKEN_COMMENT | sys::PM_TOKEN_EMBDOC_BEGIN)
                || t1.end != t2.start
                || !helpers::parser_tokens::same_line(src, t1, t2)
            {
                continue;
            }
            // Prism's comment token includes the newline; the parser gem's comment does not.
            let end = src.comments.iter().find(|c| c.start == t2.start).map_or(t2.end, |c| c.end);
            cx.add(t2.start, end, "Put a space before an end-of-line comment.");
        }
    }
}
