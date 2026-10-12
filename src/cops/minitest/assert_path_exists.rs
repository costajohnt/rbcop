use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertPathExists";
pub static COP: AssertPathExists = AssertPathExists;
pub struct AssertPathExists;

impl Cop for AssertPathExists {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        path_exists(cx, node, "assert");
    }
}
