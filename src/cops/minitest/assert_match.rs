use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertMatch";
pub static COP: AssertMatch = AssertMatch;
pub struct AssertMatch;

impl Cop for AssertMatch {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        compare_cop(cx, node, "assert_match", &[Form::Bin("assert", &["match", "match?", "=~"]), Form::Op("assert_operator", "=~")], true);
    }
}
