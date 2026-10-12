use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefuteInstanceOf";
pub static COP: RefuteInstanceOf = RefuteInstanceOf;
pub struct RefuteInstanceOf;

impl Cop for RefuteInstanceOf {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        instance_of(cx, node, "refute");
    }
}
