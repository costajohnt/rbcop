use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertPredicate";
pub static COP: AssertPredicate = AssertPredicate;
pub struct AssertPredicate;

impl Cop for AssertPredicate {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        predicate(cx, node, "assert");
    }
}
