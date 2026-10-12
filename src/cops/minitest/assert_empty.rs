use crate::prelude::*;

pub const NAME: &str = "Minitest/AssertEmpty";
pub static COP: AssertEmpty = AssertEmpty;
pub struct AssertEmpty;

impl Cop for AssertEmpty {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::cop_rule(cx, node, "assert", &[], "assert_empty", false);
    }
}
