use crate::helpers::magic_comment;
use crate::prelude::*;

pub const NAME: &str = "Lint/OrderedMagicComments";
pub static COP: OrderedMagicComments = OrderedMagicComments;
pub struct OrderedMagicComments;

impl Cop for OrderedMagicComments {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        if src.text.is_empty() {
            return;
        }
        // FrozenStringLiteral#leading_comment_lines
        let leading = match cx.facts.first_code {
            Some(off) => &src.lines[..(src.line_of(off) - 1).min(src.lines.len())],
            None => &src.lines[..],
        };
        let (mut encoding, mut other) = (None, None);
        for (i, line) in leading.iter().enumerate() {
            if magic_comment::encoding_specified(line) {
                encoding = Some(i);
            } else if magic_comment::valid(line) {
                other = Some(i);
            }
            if encoding.is_some() && other.is_some() {
                break;
            }
        }
        let (Some(enc), Some(oth)) = (encoding, other) else { return };
        if enc < oth {
            return;
        }
        let range = |i: usize| {
            let b = src.line_start(i + 1);
            (b, b + src.lines[i].len())
        };
        let (r1, r2) = (range(enc), range(oth));
        let msg = "The encoding magic comment should precede all other magic comments.";
        cx.add_fix(r1.0, r1.1, msg, |c| {
            c.replace(r1.0, r1.1, src.slice(r2.0, r2.1));
            c.replace(r2.0, r2.1, src.slice(r1.0, r1.1));
        });
    }
}
