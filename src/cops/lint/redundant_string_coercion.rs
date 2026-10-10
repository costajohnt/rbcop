use crate::prelude::*;

pub const NAME: &str = "Lint/RedundantStringCoercion";
pub static COP: RedundantStringCoercion = RedundantStringCoercion;
pub struct RedundantStringCoercion;

/// `(call _ :to_s)` (no arguments, no block) or `method?(:to_s) && arguments.none?` on a call child.
fn to_s_without_args<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    let call = node.as_call_node()?;
    (call.name().as_slice() == b"to_s" && call.arguments().is_none() && call.block().is_none()).then_some(call)
}

fn register(cx: &mut Cx, call: &ruby_prism::CallNode, context: &str) {
    let Some(sel) = call.message_loc() else { return };
    let receiver = call.receiver().map(|r| node_text(&r).to_string());
    let msg = match receiver {
        Some(_) => format!("Redundant use of `Object#to_s` in {context}."),
        None => format!("Use `self` instead of `Object#to_s` in {context}."),
    };
    let l = call.location();
    let (start, end) = (l.start_offset(), l.end_offset());
    cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| {
        c.replace(start, end, receiver.as_deref().unwrap_or("self"));
    });
}

impl Cop for RedundantStringCoercion {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::EMBEDDED_STATEMENTS_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(e) = node.as_embedded_statements_node() {
            let Some(last) = e.statements().and_then(|s| s.body().iter().last()) else { return };
            if let Some(call) = to_s_without_args(&last) {
                register(cx, &call, "interpolation");
            }
            return;
        }
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "print" | "puts" | "warn") || call.receiver().is_some() {
            return;
        }
        let context = format!("`{name}`");
        for arg in call_args(&call) {
            if let Some(child) = to_s_without_args(&arg) {
                register(cx, &child, &context);
            }
        }
    }
}
