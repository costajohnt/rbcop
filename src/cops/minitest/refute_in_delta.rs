use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefuteInDelta";
pub static COP: RefuteInDelta = RefuteInDelta;
pub struct RefuteInDelta;

impl Cop for RefuteInDelta {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        in_delta(cx, node, "refute_equal", "refute");
    }
}
