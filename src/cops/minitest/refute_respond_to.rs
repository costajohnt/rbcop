use crate::prelude::*;

pub const NAME: &str = "Minitest/RefuteRespondTo";
pub static COP: RefuteRespondTo = RefuteRespondTo;
pub struct RefuteRespondTo;

impl Cop for RefuteRespondTo {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::cop_rule(cx, node, "refute", &["respond_to?"], "refute_respond_to", false);
    }
}
