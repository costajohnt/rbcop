use crate::prelude::*;

pub const NAME: &str = "Rails/Inquiry";
pub static COP: Inquiry = Inquiry;
pub struct Inquiry;

impl Cop for Inquiry {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "inquiry" || !send_args(&call).is_empty() {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        // receiver.type?(:str, :array)
        let str = recv.as_string_node().is_some() && !is_parser_dstr(&recv);
        if !str && recv.as_array_node().is_none() {
            return;
        }
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), "Prefer Ruby's comparison operators over Active Support's `inquiry`.");
    }
}
