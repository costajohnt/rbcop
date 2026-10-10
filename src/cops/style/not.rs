use crate::helpers::node_types::{is_assignment, is_binary_operation};
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/Not";
pub static COP: Not = Not;
pub struct Not;

impl Cop for Not {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "!" || is_csend(&call) {
            return;
        }
        let Some(sel) = call.message_loc().filter(|l| text(l) == "not") else { return };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let Some(child) = call.receiver() else {
            return cx.add(s, e, "Use `!` instead of `not`.");
        };
        let side = Surrounding { side: Side::Right, ..Default::default() };
        let range = range_with_surrounding_space(cx.src.text, s, e, side);
        let child_call = child.as_call_node().filter(|c| !is_csend(c));
        let opposite = child_call.as_ref().and_then(|c| {
            let op = OPPOSITE_METHODS.iter().find(|(m, _)| *m == method_name(c))?.1;
            let sel = c.message_loc()?;
            Some((sel.start_offset(), sel.end_offset(), op))
        });
        let parens = matches!(kind::of(&child), kind::AND_NODE | kind::OR_NODE | kind::RANGE_NODE | kind::FLIP_FLOP_NODE)
            || child_call.as_ref().is_some_and(is_binary_operation)
            || child.as_if_node().is_some_and(|i| i.if_keyword_loc().is_none())
            || is_assignment(&child);
        let node_end = send_range(&call).1;
        cx.add_fix(s, e, "Use `!` instead of `not`.", |c| {
            if let Some((a, b, op)) = opposite {
                c.remove(range.0, range.1);
                c.replace(a, b, op);
            } else if parens {
                c.replace(range.0, range.1, "!(");
                c.insert_after(call.location().start_offset(), node_end, ")");
            } else {
                c.replace(range.0, range.1, "!");
            }
        });
    }
}

const OPPOSITE_METHODS: &[(&str, &str)] = &[("==", "!="), ("!=", "=="), ("<=", ">"), (">", "<="), ("<", ">="), (">=", "<")];
