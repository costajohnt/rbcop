use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertSilent";
pub static COP: AssertSilent = AssertSilent;
pub struct AssertSilent;

impl Cop for AssertSilent {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = block_call(node) else { return };
        let Some(args) = receiverless(&call, &["assert_output"]) else { return };
        let empty = |n: &Node| {
            n.as_string_node().is_some_and(|s| s.unescaped().is_empty()) && !is_parser_dstr(n)
        };
        if args.len() != 2 || !empty(&args[0]) || !empty(&args[1]) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Prefer using `assert_silent`.", |c| c.replace(s, e, "assert_silent"));
    }
}
