use crate::prelude::*;

pub const NAME: &str = "Minitest/AssertNil";
pub static COP: AssertNil = AssertNil;
pub struct AssertNil;

impl Cop for AssertNil {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::nil_assertion(cx, node, "assert");
    }
}
