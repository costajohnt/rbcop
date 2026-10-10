use crate::prelude::*;

pub const NAME: &str = "Lint/FlipFlop";
pub static COP: FlipFlop = FlipFlop;
pub struct FlipFlop;

impl Cop for FlipFlop {
    fn kinds(&self) -> &'static [usize] {
        &[kind::FLIP_FLOP_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = node.location();
        cx.add(l.start_offset(), l.end_offset(), "Avoid the use of flip-flop operators.");
    }
}
