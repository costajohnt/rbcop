use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefuteEqual";
pub static COP: RefuteEqual = RefuteEqual;
pub struct RefuteEqual;

impl Cop for RefuteEqual {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let forms = [
            Form::Bin("assert", &["!="]),
            Form::Bin("refute", &["=="]),
            Form::Op("assert_operator", "!="),
            Form::Op("refute_operator", "=="),
        ];
        compare_cop(cx, node, "refute_equal", &forms, false);
    }
}
