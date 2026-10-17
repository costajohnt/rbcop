use crate::prelude::*;

pub const NAME: &str = "RSpec/Be";
pub static COP: Be = Be;
pub struct Be;

impl Cop for Be {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ #Runners.all $(send nil? :be))
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !helpers::rspec::is_runner(method_name(&call)) {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        let Some(be) = helpers::rspec::bare_send(arg, "be") else { return };
        let sel = be.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), "Don't use `be` without an argument.");
    }
}
