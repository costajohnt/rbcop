use crate::prelude::*;

pub const NAME: &str = "Minitest/AssertKindOf";
pub static COP: AssertKindOf = AssertKindOf;
pub struct AssertKindOf;

impl Cop for AssertKindOf {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::cop_rule(cx, node, "assert", &["kind_of?", "is_a?"], "assert_kind_of", true);
    }
}
