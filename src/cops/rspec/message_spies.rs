use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/MessageSpies";
pub static COP: MessageSpies = MessageSpies;
pub struct MessageSpies;

impl Cop for MessageSpies {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send nil? :expect $_) #Runners.all ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !helpers::rspec::is_runner(method_name(&call)) {
            return;
        }
        let Some(expect) = call.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect")) else { return };
        let [receiver] = &send_args(&expect)[..] else { return };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("have_received").to_string();
        let msg = match style.as_str() {
            "receive" => "Prefer `receive` for setting message expectations.".to_string(),
            "have_received" => format!(
                "Prefer `have_received` for setting message expectations. Setup `{}` as a spy using `allow` or `instance_spy`.",
                node_text(receiver)
            ),
            _ => return,
        };
        // receive_message: $(send nil? {:receive :have_received} ...) in pre-order; the first one in
        // the preferred style ends the check
        struct V<'a> {
            style: &'a str,
            stop: bool,
            found: Vec<(usize, usize)>,
        }
        impl<'pr> Visit<'pr> for V<'_> {
            fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
                if self.stop {
                    return;
                }
                let name = method_name(n);
                if n.receiver().is_none() && matches!(name, "receive" | "have_received") {
                    if name == self.style {
                        self.stop = true;
                        return;
                    }
                    let sel = n.message_loc().unwrap();
                    self.found.push((sel.start_offset(), sel.end_offset()));
                }
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V { style: &style, stop: false, found: Vec::new() };
        // the send node only: a `do ... end` block around it is not searched
        if let Some(r) = call.receiver() {
            v.visit(&r);
        }
        for a in send_args(&call) {
            v.visit(&a);
        }
        for (s, e) in v.found {
            cx.add(s, e, msg.clone());
        }
    }
}
