use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/MessageExpectation";
pub static COP: MessageExpectation = MessageExpectation;
pub struct MessageExpectation;

/// def_node_search receive_message?: `(send nil? :receive ...)` in the node.
fn receive_message(node: &Node) -> bool {
    struct V(bool);
    impl<'pr> Visit<'pr> for V {
        fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
            if n.receiver().is_none() && method_name(n) == "receive" {
                self.0 = true;
            }
            ruby_prism::visit_call_node(self, n);
        }
    }
    let mut v = V(false);
    v.visit(node);
    v.0
}

impl Cop for MessageExpectation {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send $(send nil? {:expect :allow} ...) :to #receive_message?)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "to" {
            return;
        }
        let Some(m) = call.receiver().and_then(|r| helpers::rspec::send_call(&r)) else { return };
        if m.receiver().is_some() || !matches!(method_name(&m), "expect" | "allow") {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        if !receive_message(arg) {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("allow");
        if method_name(&m) == style {
            return;
        }
        let sel = m.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), format!("Prefer `{style}` for setting message expectations."));
    }
}
