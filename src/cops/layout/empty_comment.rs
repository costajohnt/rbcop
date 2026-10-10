use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyComment";
pub static COP: EmptyComment = EmptyComment;
pub struct EmptyComment;

impl Cop for EmptyComment {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let border = cx.cfg.bool("AllowBorderComment").unwrap_or(true);
        let margin = cx.cfg.bool("AllowMarginComment").unwrap_or(true);
        // empty_comment_only? on one comment's stripped text: `#` (or `###` without AllowBorderComment).
        let empty = |c: &Comment| {
            let t = helpers::ruby_string::strip(src.slice(c.start, c.end));
            !t.is_empty() && t.bytes().all(|b| b == b'#') && (!border || t.len() == 1)
        };
        let comments = &src.comments;
        let mut i = 0;
        while i < comments.len() {
            // AllowMarginComment: a run of comments on consecutive lines in the same column is
            // only empty if every comment in it is.
            let mut j = i + 1;
            if margin {
                while j < comments.len() && {
                    let (a, b) = (&comments[j - 1], &comments[j]);
                    src.line_of(a.start) + 1 == src.line_of(b.start) && src.column(a.start) == src.column(b.start)
                } {
                    j += 1;
                }
            }
            if comments[i..j].iter().all(empty) {
                for c in &comments[i..j] {
                    let end = if src.text[..c.end].ends_with('\n') { c.end - 1 } else { c.end };
                    // inline_comment?: code before the comment on its line.
                    let line_start = src.line_start(src.line_of(c.start));
                    let range = if helpers::ruby_string::strip(&src.text[line_start..c.start]).is_empty() {
                        helpers::whole_lines::range_by_whole_lines(src.text, c.start, end, true)
                    } else {
                        let o = helpers::range_help::Surrounding { newlines: false, ..Default::default() };
                        helpers::range_help::range_with_surrounding_space(src.text, c.start, end, o)
                    };
                    cx.add_fix(c.start, end, "Source code comment is empty.", |c| c.remove(range.0, range.1));
                }
            }
            i = j;
        }
    }
}
