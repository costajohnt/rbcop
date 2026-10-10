use crate::prelude::*;

pub const NAME: &str = "Lint/RequireParentheses";
pub static COP: RequireParentheses = RequireParentheses;
pub struct RequireParentheses;

const MSG: &str = "Use parentheses in the method call to avoid confusion about precedence.";

fn operator_keyword(node: &Node) -> bool {
    matches!(kind::of(node), kind::AND_NODE | kind::OR_NODE)
}

impl Cop for RequireParentheses {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // parser arguments include a `&blk` block pass.
        let mut args = call_args(&call);
        if let Some(b) = call.block().filter(|b| b.as_block_argument_node().is_some()) {
            args.push(b);
        }
        if args.is_empty() || call.opening_loc().is_some_and(|l| l.as_slice() == b"(") {
            return;
        }
        let name = method_name(&call);
        let (start, end) = send_range(&call);
        if let Some(ternary) = args[0].as_if_node().filter(|i| i.if_keyword_loc().is_none()) {
            let assignment = name.ends_with('=') && !matches!(name, "==" | "===" | "!=" | "<=" | ">=");
            let cond = ternary.predicate();
            if name == "[]" || assignment || !operator_keyword(&cond) {
                return;
            }
            cx.add(start, cond.location().end_offset(), MSG);
        } else if name.ends_with('?') && operator_keyword(args.last().unwrap()) {
            cx.add(start, end, MSG);
        }
    }
}
