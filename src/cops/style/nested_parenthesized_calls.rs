use crate::helpers::range_help::{range_with_surrounding_space, Side, Surrounding};
use crate::prelude::*;

pub const NAME: &str = "Style/NestedParenthesizedCalls";
pub static COP: NestedParenthesizedCalls = NestedParenthesizedCalls;
pub struct NestedParenthesizedCalls;

impl Cop for NestedParenthesizedCalls {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.opening_loc().is_none_or(|o| o.as_slice() != b"(") {
            return;
        }
        let outer_args = send_args(&call);
        // each_child_node(:call): the receiver and the arguments that are parser send/csend nodes.
        for nested in call.receiver().into_iter().chain(call_args(&call)) {
            let Some(inner) = nested.as_call_node() else { continue };
            if inner.block().is_some_and(|b| b.as_block_node().is_some()) {
                continue;
            }
            let args = send_args(&inner);
            let allowed = args.is_empty()
                || inner.opening_loc().is_some_and(|o| o.as_slice() == b"(")
                || inner.equal_loc().is_some()
                || helpers::node_types::is_operator_method(method_name(&inner))
                || (outer_args.len() == 1
                    && helpers::allowed::allowed_method(cx.cfg, method_name(&inner))
                    && args.len() == 1);
            if allowed {
                continue;
            }
            let (start, end) = (nested.location().start_offset(), nested.location().end_offset());
            let msg = format!("Add parentheses to nested method call `{}`.", node_text(&nested));
            let first = args[0].location().start_offset();
            let last_loc = args[args.len() - 1].location();
            let (last_start, last) = (last_loc.start_offset(), last_loc.end_offset());
            let opts = Surrounding { side: Side::Left, whitespace: true, continuations: true, ..Default::default() };
            let (lead, _) = range_with_surrounding_space(cx.src.text, first, first, opts);
            cx.add_fix(start, end, &msg, |c| {
                c.replace(lead, first, "(");
                c.insert_after(last_start, last, ")");
            });
        }
    }
}
