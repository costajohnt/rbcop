use crate::prelude::*;

pub const NAME: &str = "RSpec/MessageChain";
pub static COP: MessageChain = MessageChain;
pub struct MessageChain;

impl Cop for MessageChain {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "receive_message_chain" | "stub_chain") {
            return;
        }
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), format!("Avoid stubbing using `{name}`."));
    }
}
