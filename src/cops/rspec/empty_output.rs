use crate::prelude::*;

pub const NAME: &str = "RSpec/EmptyOutput";
pub static COP: EmptyOutput = EmptyOutput;
pub struct EmptyOutput;

impl Cop for EmptyOutput {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (block (send nil? :expect) ...) #Runners.all (send $(send nil? :output (str empty?)) ...))
        let send = node.as_call_node().unwrap();
        if is_csend(&send) || !helpers::rspec::is_runner(method_name(&send)) {
            return;
        }
        let Some(recv) = send.receiver().and_then(|r| helpers::rspec::plain_block_call(&r)) else { return };
        if recv.receiver().is_some() || method_name(&recv) != "expect" || !send_args(&recv).is_empty() {
            return;
        }
        let [arg] = &send_args(&send)[..] else { return };
        let Some(outer) = arg.as_call_node().filter(|c| !is_csend(c) && helpers::rspec::block_of(c).is_none()) else {
            return;
        };
        let Some(output) = outer.receiver().and_then(|r| helpers::rspec::nil_send(&r, "output")) else { return };
        let [s] = &send_args(&output)[..] else { return };
        if !helpers::rspec::is_str(s) || !s.as_string_node().unwrap().unescaped().is_empty() {
            return;
        }
        let runner = if method_name(&send) == "to" { "not_to" } else { "to" };
        let sel = send.message_loc().unwrap();
        let (rs, re) = (sel.start_offset(), sel.end_offset());
        let (os, oe) = helpers::rspec::node_range(&output.as_node());
        cx.add_fix(os, oe, format!("Use `{runner}` instead of matching on an empty output."), |c| {
            c.replace(rs, re, runner);
            c.replace(os, oe, "output");
        });
    }
}
