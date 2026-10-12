use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertSame";
pub static COP: AssertSame = AssertSame;
pub struct AssertSame;

impl Cop for AssertSame {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        same_cop(cx, node, "assert");
    }
}
