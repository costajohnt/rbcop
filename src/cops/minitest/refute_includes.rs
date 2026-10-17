use crate::prelude::*;

pub const NAME: &str = "Minitest/RefuteIncludes";
pub static COP: RefuteIncludes = RefuteIncludes;
pub struct RefuteIncludes;

impl Cop for RefuteIncludes {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::cop_rule(cx, node, "refute", &["include?", "member?"], "refute_includes", false);
    }
}
