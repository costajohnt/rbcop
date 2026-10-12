use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertInstanceOf";
pub static COP: AssertInstanceOf = AssertInstanceOf;
pub struct AssertInstanceOf;

impl Cop for AssertInstanceOf {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        instance_of(cx, node, "assert");
    }
}
