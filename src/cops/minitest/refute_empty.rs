use crate::prelude::*;

pub const NAME: &str = "Minitest/RefuteEmpty";
pub static COP: RefuteEmpty = RefuteEmpty;
pub struct RefuteEmpty;

impl Cop for RefuteEmpty {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::cop_rule(cx, node, "refute", &[], "refute_empty", false);
    }
}
