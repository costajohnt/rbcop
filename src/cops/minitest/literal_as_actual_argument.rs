use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/LiteralAsActualArgument";
pub static COP: LiteralAsActualArgument = LiteralAsActualArgument;
pub struct LiteralAsActualArgument;

impl Cop for LiteralAsActualArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "assert_equal" || is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        let (Some(expected), Some(actual)) = (args.first(), args.get(1)) else { return };
        use helpers::literal_node::recursive_basic_literal;
        if !recursive_basic_literal(actual) || recursive_basic_literal(expected) {
            return;
        }
        let hash = matches!(actual, Node::HashNode { .. } | Node::KeywordHashNode { .. });
        let new_actual = if actual.as_keyword_hash_node().is_some() {
            format!("{{{}}}", node_text(actual))
        } else {
            node_text(actual).to_string()
        };
        let (er, ar) = (range(expected), range(actual));
        let expected_src = node_text(expected);
        let parens = call.opening_loc().is_some_and(|o| o.as_slice() == b"(");
        let sel_end = call.message_loc().unwrap().end_offset();
        let send_end = send_range(&call).1;
        let r = all_arguments(&args);
        cx.add_fix(r.0, r.1, "Replace the literal with the first argument.", |c| {
            c.replace(er.0, er.1, &new_actual);
            c.replace(ar.0, ar.1, expected_src);
            if !parens && hash {
                c.replace(sel_end, er.0, "(");
                c.insert_after(send_end, send_end, ")");
            }
        });
    }
}
