use crate::prelude::*;

pub const NAME: &str = "ThreadSafety/ActiveSupportCallbacks";
pub static COP: ActiveSupportCallbacks = ActiveSupportCallbacks;
pub struct ActiveSupportCallbacks;

/// `receiver = receiver.children.first while receiver&.begin_type? && receiver.children.one?`, then `const_type?`.
fn constant_receiver(node: &Node) -> bool {
    if let Some(body) = node.as_parentheses_node().and_then(|p| p.body()) {
        let Some(s) = body.as_statements_node() else { return constant_receiver(&body) };
        let mut it = s.body().iter();
        return match (it.next(), it.next()) {
            (Some(only), None) => constant_receiver(&only),
            _ => false,
        };
    }
    matches!(node, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. })
}

impl Cop for ActiveSupportCallbacks {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "set_callback" | "skip_callback") {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        // requires_gem 'activesupport'
        if !constant_receiver(&recv) || !cx.config.gem_at_least("activesupport", "0") {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let dot = call.call_operator_loc().map_or("", |l| text(&l));
        let msg = format!(
            "Avoid process-wide ActiveSupport callback mutation with `{}{dot}{}`.",
            node_text(&recv),
            method_name(&call)
        );
        cx.add(sel.start_offset(), sel.end_offset(), msg);
    }
}
