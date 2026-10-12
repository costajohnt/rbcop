use crate::prelude::*;

pub const NAME: &str = "Performance/OpenStruct";
pub static COP: OpenStruct = OpenStruct;
pub struct OpenStruct;

impl Cop for OpenStruct {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? cbase} :OpenStruct) :new ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "new" {
            return;
        }
        if !call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "OpenStruct")) {
            return;
        }
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), "Consider using `Struct` over `OpenStruct` to optimize the performance.");
    }
}
