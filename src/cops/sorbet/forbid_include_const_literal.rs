use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidIncludeConstLiteral";
pub static COP: ForbidIncludeConstLiteral = ForbidIncludeConstLiteral;
pub struct ForbidIncludeConstLiteral;

/// `within_onymous_module?`: the nearest parser ancestor that is not a `begin` or `block` is a
/// class, module or sclass.
fn within_onymous_module(ancestors: &[Node]) -> bool {
    for i in (0..ancestors.len()).rev() {
        let n = &ancestors[i];
        let f = frame(n);
        if f == Frame::Transparent || (f == Frame::Call && ancestors.get(i + 1).is_some_and(|b| frame(b) == Frame::Block)) {
            continue;
        }
        match n {
            Node::StatementsNode { .. } | Node::ParenthesesNode { .. } => continue,
            Node::BlockNode { .. } if helpers::node_types::plain_block_params(n.as_block_node().unwrap().parameters()) => continue,
            Node::LambdaNode { .. } if helpers::node_types::plain_block_params(n.as_lambda_node().unwrap().parameters()) => continue,
            Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. } => return true,
            _ => return false,
        }
    }
    false
}

impl Cop for ForbidIncludeConstLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? ${:include :extend :prepend} $#neither_const_nor_self?)
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "include" | "extend" | "prepend") || call.receiver().is_some() || is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        if matches!(arg, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } | Node::SelfNode { .. }) {
            return;
        }
        // A literal block makes the send's parent that block (a numbered one is no `block`).
        if let Some(b) = call.block().and_then(|b| b.as_block_node())
            && !helpers::node_types::plain_block_params(b.parameters())
        {
            return;
        }
        if !within_onymous_module(cx.ancestors) {
            return;
        }
        let replacement = format!("T.unsafe(self).{name} {}", node_text(arg));
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, format!("`{name}` must only be used with constant literals as arguments"), |c| {
            c.replace(s, e, &replacement)
        });
    }
}
