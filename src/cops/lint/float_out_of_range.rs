use crate::prelude::*;

pub const NAME: &str = "Lint/FloatOutOfRange";
pub static COP: FloatOutOfRange = FloatOutOfRange;
pub struct FloatOutOfRange;

impl Cop for FloatOutOfRange {
    fn kinds(&self) -> &'static [usize] {
        &[kind::FLOAT_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let v = node.as_float_node().unwrap().value();
        let src = node_text(node);
        if v.is_infinite() || (v == 0.0 && src.bytes().any(|b| (b'1'..=b'9').contains(&b))) {
            let l = node.location();
            cx.add(l.start_offset(), l.end_offset(), "Float out of range.");
        }
    }
}
