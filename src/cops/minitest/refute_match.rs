use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefuteMatch";
pub static COP: RefuteMatch = RefuteMatch;
pub struct RefuteMatch;

impl Cop for RefuteMatch {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let forms = [
            Form::Bin("refute", &["match", "match?", "=~"]),
            Form::Op("refute_operator", "=~"),
            Form::Op("assert_operator", "!~"),
        ];
        compare_cop(cx, node, "refute_match", &forms, true);
    }
}
