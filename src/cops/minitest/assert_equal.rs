use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertEqual";
pub static COP: AssertEqual = AssertEqual;
pub struct AssertEqual;

impl Cop for AssertEqual {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        compare_cop(cx, node, "assert_equal", &[Form::Bin("assert", &["=="]), Form::Op("assert_operator", "==")], false);
    }
}
