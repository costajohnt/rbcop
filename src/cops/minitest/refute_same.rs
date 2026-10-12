use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefuteSame";
pub static COP: RefuteSame = RefuteSame;
pub struct RefuteSame;

impl Cop for RefuteSame {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        same_cop(cx, node, "refute");
    }
}
