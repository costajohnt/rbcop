use crate::helpers::parser_parent::{is_top_const, parser_parent};
use crate::helpers::file_open_patterns::{is_sym_block_pass, mode_str, single_arg_block_send};
use crate::prelude::*;

pub const NAME: &str = "Style/FileRead";
pub static COP: FileRead = FileRead;
pub struct FileRead;

const MODES: &[&[u8]] = &[b"r", b"rt", b"rb", b"r+", b"r+t", b"r+b"];

impl Cop for FileRead {
    fn correctable(&self) -> bool {
        true
    }

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
        let args = send_args(&call);
        let Some(filename) = args.first() else { return };
        let mut rest = &args[1..];
        let mut mode: &[u8] = b"r";
        if let Some(m) = rest.first().and_then(|a| mode_str(a, MODES)) {
            mode = m;
            rest = &rest[1..];
        }
        let block_pass = match rest {
            [] => false,
            [b] if is_sym_block_pass(b, "read") => true,
            _ => return,
        };
        let (start, _) = send_range(&call);
        let read_end = if block_pass {
            send_range(&call).1
        } else if single_arg_block_send(&call, "read").is_some_and(|s| send_args(&s).is_empty()) {
            node.location().end_offset()
        } else if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        } else {
            // `(send _ :read)` parent.
            let Some(p) = parser_parent(cx.ancestors).and_then(|p| p.as_call_node()) else { return };
            if is_csend(&p) || method_name(&p) != "read" || !send_args(&p).is_empty() {
                return;
            }
            send_range(&p).1
        };
        let method = if mode.ends_with(b"b") { "binread" } else { "read" };
        let sel = call.message_loc().unwrap().start_offset();
        let replacement = format!("{method}({})", node_text(filename));
        cx.add_fix(start, read_end, format!("Use `File.{method}`."), |c| c.replace(sel, read_end, &replacement));
    }
}
