use crate::prelude::*;

pub const NAME: &str = "Minitest/AssertRespondTo";
pub static COP: AssertRespondTo = AssertRespondTo;
pub struct AssertRespondTo;

impl Cop for AssertRespondTo {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::cop_rule(cx, node, "assert", &["respond_to?"], "assert_respond_to", false);
    }
}
