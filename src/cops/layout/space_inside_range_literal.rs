use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideRangeLiteral";
pub static COP: SpaceInsideRangeLiteral = SpaceInsideRangeLiteral;
pub struct SpaceInsideRangeLiteral;

/// `whitespace_range`: the range when it is all (ASCII, Ruby `\s`) whitespace.
fn whitespace_range(text: &str, begin: usize, end: usize) -> Option<(usize, usize)> {
    let s = text.get(begin..end)?;
    (!s.is_empty() && s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')))
        .then_some((begin, end))
}

impl Cop for SpaceInsideRangeLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RANGE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let range = node.as_range_node().unwrap();
        // Below 2.7 a beginless range is Prism's reading of `0.. ..3`, the parser gem's `(0..)..3`
        // (legacy_syntax), whose inner range is `0..`.
        // ponytail: other cops still see Prism's shape there.
        if range.left().is_none() && cx.src.target_ruby < 2.7 {
            return;
        }
        let text = cx.src.text;
        let op = range.operator_loc();
        let (op_begin, op_end) = (op.start_offset(), op.end_offset());
        let before = range.left().and_then(|l| whitespace_range(text, l.location().end_offset(), op_begin));
        let after = range.right().and_then(|r| whitespace_range(text, op_end, r.location().start_offset()));
        let spaces: Vec<(usize, usize)> = before.into_iter().chain(after).collect();
        // A line break right after the operator (plus the next line's indentation) is fine.
        if spaces.iter().all(|&(b, _)| b == op_end && text.as_bytes()[b] == b'\n') {
            return;
        }
        let loc = node.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), "Space inside range literal.", |c| {
            for &(b, e) in &spaces {
                c.remove(b, e);
            }
        });
    }
}
