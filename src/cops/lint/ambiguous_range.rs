use crate::prelude::*;
use helpers::literal_node::{basic_literal, literal};
use helpers::node_types::{is_operator_method, is_unary_operation};

pub const NAME: &str = "Lint/AmbiguousRange";
pub static COP: AmbiguousRange = AmbiguousRange;
pub struct AmbiguousRange;

const MSG: &str = "Wrap complex range boundaries with parentheses to avoid ambiguity.";

impl Cop for AmbiguousRange {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RANGE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let range = node.as_range_node().unwrap();
        let chains = cx.cfg.bool("RequireParenthesesForMethodChains").unwrap_or(false);
        for boundary in [range.left(), range.right()].into_iter().flatten() {
            if acceptable(&boundary, chains) {
                continue;
            }
            let (s, e) = helpers::literal_node::expression_range(&boundary);
            cx.add_fix(s, e, MSG, |c| c.wrap(s, e, "(", ")"));
        }
    }
}

fn acceptable(node: &Node, chains: bool) -> bool {
    match node {
        Node::ParenthesesNode { .. }
        | Node::LocalVariableReadNode { .. }
        | Node::ItLocalVariableReadNode { .. }
        | Node::InstanceVariableReadNode { .. }
        | Node::ClassVariableReadNode { .. }
        | Node::GlobalVariableReadNode { .. }
        | Node::ConstantReadNode { .. }
        | Node::ConstantPathNode { .. }
        | Node::SelfNode { .. } => true,
        _ if literal(node) || rational_literal(node) => true,
        // `call_type?`: a call with a literal block is a parser `block`.
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            call.block().is_none_or(|b| b.as_block_node().is_none()) && acceptable_call(&call, chains)
        }
        _ => false,
    }
}

fn acceptable_call(call: &ruby_prism::CallNode, chains: bool) -> bool {
    if is_unary_operation(call) {
        return true;
    }
    if call.receiver().is_some_and(|r| basic_literal(&r)) {
        return false;
    }
    let name = method_name(call);
    if is_operator_method(name) && name != "[]" {
        return false;
    }
    !chains || call.receiver().is_none()
}

/// RationalLiteral: `(send (int _) :/ (rational _))`
fn rational_literal(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        method_name(&c) == "/"
            && !is_csend(&c)
            && c.receiver().is_some_and(|r| r.as_integer_node().is_some())
            && matches!(call_args(&c).as_slice(), [a] if a.as_rational_node().is_some())
            && c.block().is_none()
    })
}
