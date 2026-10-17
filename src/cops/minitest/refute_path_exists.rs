use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefutePathExists";
pub static COP: RefutePathExists = RefutePathExists;
pub struct RefutePathExists;

impl Cop for RefutePathExists {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        path_exists(cx, node, "refute");
    }
}
