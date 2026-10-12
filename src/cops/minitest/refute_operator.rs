use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefuteOperator";
pub static COP: RefuteOperator = RefuteOperator;
pub struct RefuteOperator;

impl Cop for RefuteOperator {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        operator(cx, node, "refute");
    }
}
