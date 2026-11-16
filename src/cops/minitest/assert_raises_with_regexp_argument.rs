use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertRaisesWithRegexpArgument";
pub static COP: AssertRaisesWithRegexpArgument = AssertRaisesWithRegexpArgument;
pub struct AssertRaisesWithRegexpArgument;

impl Cop for AssertRaisesWithRegexpArgument {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn restrict_on_send(&self) -> &'static [&'static str] {
        &["assert_raises"]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "assert_raises" || !send_args(&call).last().is_some_and(is_regexp) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Do not pass regular expression literals to `assert_raises`. Test the resulting exception.");
    }
}
