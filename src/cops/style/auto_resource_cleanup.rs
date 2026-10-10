use crate::helpers::parser_parent::{is_top_const, parser_parent};
use crate::prelude::*;

pub const NAME: &str = "Style/AutoResourceCleanup";
pub static COP: AutoResourceCleanup = AutoResourceCleanup;
pub struct AutoResourceCleanup;

impl Cop for AutoResourceCleanup {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? cbase} {:File :Tempfile}) :open ...)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "open" || is_csend(&call) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        if !is_top_const(&recv, "File") && !is_top_const(&recv, "Tempfile") {
            return;
        }
        // cleanup?: a block pass, a block, or any parent but a local variable assignment
        if call.block().is_some() {
            return;
        }
        if parser_parent(cx.ancestors).is_some_and(|p| p.as_local_variable_write_node().is_none()) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let current = cx.src.slice(recv.location().start_offset(), sel.end_offset());
        let (start, end) = send_range(&call);
        cx.add(start, end, format!("Use the block version of `{current}`."));
    }
}
