use crate::prelude::*;

pub const NAME: &str = "Minitest/RefuteNil";
pub static COP: RefuteNil = RefuteNil;
pub struct RefuteNil;

impl Cop for RefuteNil {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::nil_assertion(cx, node, "refute");
    }
}
