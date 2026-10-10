use crate::prelude::*;

pub const NAME: &str = "Style/Send";
pub static COP: Send = Send;
pub struct Send;

impl Cop for Send {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "send" || send_args(&call).is_empty() {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        cx.add(sel.start_offset(), sel.end_offset(), "Prefer `Object#__send__` or `Object#public_send` to `send`.");
    }
}
