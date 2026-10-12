use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefutePredicate";
pub static COP: RefutePredicate = RefutePredicate;
pub struct RefutePredicate;

impl Cop for RefutePredicate {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        predicate(cx, node, "refute");
    }
}
