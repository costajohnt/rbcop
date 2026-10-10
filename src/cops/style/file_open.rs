use crate::helpers::parser_parent::{is_top_const, parser_parent};
use crate::prelude::*;

pub const NAME: &str = "Style/FileOpen";
pub static COP: FileOpen = FileOpen;
pub struct FileOpen;

impl Cop for FileOpen {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "open" || is_csend(&call) {
            return;
        }
        if !call.receiver().is_some_and(|r| is_top_const(&r, "File")) {
            return;
        }
        match call.block() {
            // block_argument?
            Some(b) if b.as_block_argument_node().is_some() => return,
            // The send's parent is the block: its value is used, and it is no lvasgn or receiver.
            Some(_) => return,
            None => {}
        }
        let (start, end) = send_range(&call);
        let offensive = !helpers::value_used::value_used(cx.ancestors, start)
            || parser_parent(cx.ancestors).is_some_and(|p| {
                p.as_local_variable_write_node().is_some()
                    || p.as_call_node().and_then(|c| c.receiver()).is_some_and(|r| r.location().start_offset() == start)
            });
        if offensive {
            cx.add(start, end, "`File.open` without a block may leak a file descriptor; use the block form.");
        }
    }
}
