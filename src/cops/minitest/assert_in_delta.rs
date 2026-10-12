use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertInDelta";
pub static COP: AssertInDelta = AssertInDelta;
pub struct AssertInDelta;

impl Cop for AssertInDelta {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        in_delta(cx, node, "assert_equal", "assert");
    }
}
