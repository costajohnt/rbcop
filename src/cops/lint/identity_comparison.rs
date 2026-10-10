use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Lint/IdentityComparison";
pub static COP: IdentityComparison = IdentityComparison;
pub struct IdentityComparison;

/// `(send _ :object_id)` without arguments or block.
fn object_id_send<'pr>(n: Option<Node<'pr>>) -> Option<CallNode<'pr>> {
    n?.as_call_node().filter(|c| {
        !is_csend(c) && method_name(c) == "object_id" && c.arguments().is_none() && c.block().is_none()
    })
}

impl Cop for IdentityComparison {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let op = method_name(&call);
        if !matches!(op, "==" | "!=") || is_csend(&call) || call.block().is_some() {
            return;
        }
        let args = call_args(&call);
        if args.len() != 1 {
            return;
        }
        let (Some(lhs), Some(rhs)) = (object_id_send(call.receiver()), object_id_send(args.into_iter().next())) else {
            return;
        };
        // `return unless receiver && argument` inside the corrector block drops the offense.
        let (Some(recv), Some(arg)) = (lhs.receiver(), rhs.receiver()) else { return };
        let bang = if op == "==" { "" } else { "!" };
        let replacement = format!("{bang}{}.equal?({})", node_text(&recv), node_text(&arg));
        let l = node.location();
        let msg = format!("Use `{bang}equal?` instead of `{op}` when comparing `object_id`.");
        cx.add_fix(l.start_offset(), l.end_offset(), msg, |c| c.replace(l.start_offset(), l.end_offset(), &replacement));
    }
}
