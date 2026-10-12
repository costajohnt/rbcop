use crate::prelude::*;

pub const NAME: &str = "Minitest/AssertIncludes";
pub static COP: AssertIncludes = AssertIncludes;
pub struct AssertIncludes;

impl Cop for AssertIncludes {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::cop_rule(cx, node, "assert", &["include?", "member?"], "assert_includes", false);
    }
}
