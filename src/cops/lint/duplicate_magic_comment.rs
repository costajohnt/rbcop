use crate::cops::style_frozen_string_literal_comment::frozen_value;
use crate::prelude::*;

pub const NAME: &str = "Lint/DuplicateMagicComment";
pub static COP: DuplicateMagicComment = DuplicateMagicComment;
pub struct DuplicateMagicComment;

impl Cop for DuplicateMagicComment {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        if cx.src.text.is_empty() {
            return;
        }
        let (mut encoding, mut frozen) = (vec![], vec![]);
        for (i, line) in helpers::frozen_string_literal::leading_comment_lines(cx).iter().enumerate() {
            if helpers::magic_comment::encoding_specified(line) {
                encoding.push(i);
            } else if frozen_value(line).is_some() {
                frozen.push(i);
            }
        }
        let text = cx.src.text;
        for lines in [encoding, frozen] {
            for &i in lines.iter().skip(1) {
                let start = cx.src.line_start(i + 1);
                let end = start + text[start..].find('\n').unwrap_or(text.len() - start);
                let (s, e) = helpers::whole_lines::range_by_whole_lines(text, start, end, true);
                cx.add_fix(start, end, "Duplicate magic comment detected.", |c| c.remove(s, e));
            }
        }
    }
}
