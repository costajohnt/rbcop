use crate::prelude::*;

pub const NAME: &str = "Minitest/UselessAssertion";
pub static COP: UselessAssertion = UselessAssertion;
pub struct UselessAssertion;

impl Cop for UselessAssertion {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        let (expected, actual) = (args.first(), args.get(1));
        let variable = |n: &Node| {
            matches!(
                n,
                Node::LocalVariableReadNode { .. }
                    | Node::InstanceVariableReadNode { .. }
                    | Node::ClassVariableReadNode { .. }
                    | Node::GlobalVariableReadNode { .. }
            )
        };
        let empty_composite = |n: &Node| match n {
            Node::StringNode { .. } => !is_parser_dstr(n) && n.as_string_node().unwrap().unescaped().is_empty(),
            Node::ArrayNode { .. } => n.as_array_node().unwrap().elements().iter().next().is_none(),
            Node::HashNode { .. } => n.as_hash_node().unwrap().elements().iter().next().is_none(),
            _ => false,
        };
        let offense = match method_name(&call) {
            "assert" | "refute" | "assert_nil" | "refute_nil" | "assert_not" | "assert_empty" | "refute_empty" => {
                actual.is_none()
                    && expected.is_some_and(|e| {
                        helpers::literal_node::literal(e)
                            && !matches!(e, Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. })
                    })
            }
            "assert_equal" | "refute_equal" | "assert_in_delta" | "refute_in_delta" | "assert_in_epsilon"
            | "refute_in_epsilon" | "assert_same" | "refute_same" => match (expected, actual) {
                (Some(e), Some(a)) if node_text(e) == node_text(a) => {
                    (variable(e) && variable(a)) || (empty_composite(e) && empty_composite(a))
                }
                _ => false,
            },
            "assert_includes" | "refute_includes" => expected.is_some_and(empty_composite),
            "assert_silent" => match call.block().and_then(|b| b.as_block_node()) {
                Some(b) => b.body().is_none(),
                None => helpers::parser_parent::parser_parent(cx.ancestors).is_none(),
            },
            _ => false,
        };
        if offense {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Useless assertion detected.");
        }
    }
}
