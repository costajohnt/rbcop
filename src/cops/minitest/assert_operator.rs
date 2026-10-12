use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertOperator";
pub static COP: AssertOperator = AssertOperator;
pub struct AssertOperator;

impl Cop for AssertOperator {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        operator(cx, node, "assert");
    }
}
