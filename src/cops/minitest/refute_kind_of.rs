use crate::prelude::*;

pub const NAME: &str = "Minitest/RefuteKindOf";
pub static COP: RefuteKindOf = RefuteKindOf;
pub struct RefuteKindOf;

impl Cop for RefuteKindOf {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::minitest::cop_rule(cx, node, "refute", &["kind_of?", "is_a?"], "refute_kind_of", true);
    }
}
