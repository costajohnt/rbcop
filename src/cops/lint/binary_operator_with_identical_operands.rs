use crate::prelude::*;

pub const NAME: &str = "Lint/BinaryOperatorWithIdenticalOperands";
pub static COP: BinaryOperatorWithIdenticalOperands = BinaryOperatorWithIdenticalOperands;
pub struct BinaryOperatorWithIdenticalOperands;

const RESTRICT: &[&str] = &["==", "!=", "===", "<=>", "=~", ">", ">=", "<", "<=", "|", "^"];

impl Cop for BinaryOperatorWithIdenticalOperands {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::AND_NODE, kind::OR_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (lhs, rhs, op, range) = match kind::of(node) {
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                let name = method_name(&call);
                // binary_operation?: an operator method whose selector does not start the node.
                if is_csend(&call) || !RESTRICT.contains(&name) {
                    return;
                }
                let Some(sel) = call.message_loc() else { return };
                if sel.start_offset() == node.location().start_offset() {
                    return;
                }
                let (Some(recv), Some(arg)) = (call.receiver(), send_args(&call).into_iter().next()) else { return };
                (recv.location(), arg.location(), name.to_string(), send_range(&call))
            }
            kind::AND_NODE => {
                let n = node.as_and_node().unwrap();
                let l = node.location();
                (n.left().location(), n.right().location(), text(&n.operator_loc()).to_string(), (l.start_offset(), l.end_offset()))
            }
            _ => {
                let n = node.as_or_node().unwrap();
                let l = node.location();
                (n.left().location(), n.right().location(), text(&n.operator_loc()).to_string(), (l.start_offset(), l.end_offset()))
            }
        };
        let a = (lhs.start_offset(), lhs.end_offset());
        let b = (rhs.start_offset(), rhs.end_offset());
        if helpers::node_equality::same(cx.src, a, b) {
            cx.add(range.0, range.1, format!("Binary operator `{op}` has identical operands."));
        }
    }
}
