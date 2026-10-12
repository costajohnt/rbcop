use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertWithExpectedArgument";
pub static COP: AssertWithExpectedArgument = AssertWithExpectedArgument;
pub struct AssertWithExpectedArgument;

impl Cop for AssertWithExpectedArgument {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(args) = receiverless(&call, &["assert"]) else { return };
        if args.len() != 2 {
            return;
        }
        let message = &args[1];
        let string = matches!(
            message,
            Node::StringNode { .. } | Node::InterpolatedStringNode { .. } | Node::SourceFileNode { .. }
        );
        if string || matches!(node_text(message), "message" | "msg") {
            return;
        }
        let shown: Vec<&str> = args.iter().map(|a| node_text(a)).collect();
        let (s, e) = send_range(&call);
        cx.add(s, e, format!("Did you mean to use `assert_equal({})`?", shown.join(", ")));
    }
}
