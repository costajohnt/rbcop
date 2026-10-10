use crate::helpers::parser_parent::parser_parent;
use crate::prelude::*;

pub const NAME: &str = "Style/ArrayFirstLast";
pub static COP: ArrayFirstLast = ArrayFirstLast;
pub struct ArrayFirstLast;

/// A parser `send` (not csend, no literal block) of `name`.
fn plain_send(n: &Node, names: &[&str]) -> bool {
    n.as_call_node().is_some_and(|c| {
        !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none()) && names.contains(&method_name(&c))
    })
}

impl Cop for ArrayFirstLast {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "[]" || call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
            return;
        }
        let args = call_args(&call);
        if args.len() != 1 {
            return;
        }
        let preferred = match int_literal_value(&args[0]) {
            Some(0) => "first",
            Some(-1) => "last",
            _ => return,
        };
        // innermost_braces_node: a `[]` receiver makes this node the parent of a brace method
        if call.receiver().is_some_and(|r| plain_send(&r, &["[]"])) {
            return;
        }
        // brace_method?(node.parent) (a literal block is the parser parent otherwise)
        if call.block().is_none() {
            let start = node.location().start_offset();
            let brace_parent = parser_parent(cx.ancestors).is_some_and(|p| {
                if plain_send(p, &["[]", "[]="]) {
                    return true;
                }
                // `a[i] op= v` / `a[i], b = v`: parser's `send :[]` (op_asgn target) holds receiver and index
                let op = match p {
                    Node::IndexOperatorWriteNode { .. } => p.as_index_operator_write_node().map(|n| n.binary_operator_loc()),
                    Node::IndexOrWriteNode { .. } => p.as_index_or_write_node().map(|n| n.operator_loc()),
                    Node::IndexAndWriteNode { .. } => p.as_index_and_write_node().map(|n| n.operator_loc()),
                    Node::IndexTargetNode { .. } => return true,
                    _ => None,
                };
                op.is_some_and(|o| start < o.start_offset())
            });
            if brace_parent {
                return;
            }
        }
        let Some(sel) = call.message_loc() else { return };
        let dot = call.call_operator_loc().is_some();
        let (start, end) = if dot { (sel.start_offset(), send_range(&call).1) } else { (sel.start_offset(), sel.end_offset()) };
        let replacement = if dot { preferred.to_string() } else { format!(".{preferred}") };
        cx.add_fix(start, end, format!("Use `{preferred}`."), |c| c.replace(start, end, &replacement));
    }
}
