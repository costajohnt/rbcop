use crate::helpers::file_open_patterns::mode_str;
use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/FileTouch";
pub static COP: FileTouch = FileTouch;
pub struct FileTouch;

const APPEND_MODES: &[&[u8]] = &[b"a", b"a+", b"ab", b"a+b", b"at", b"a+t"];

impl Cop for FileTouch {
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
        let [filename, mode] = args.as_slice() else { return };
        if mode_str(mode, APPEND_MODES).is_none() {
            return;
        }
        // The parent is an empty `block` (not a numblock/itblock, which always have a body).
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if block.body().is_some() {
            return;
        }
        let filename = node_text(filename);
        let msg = format!("Use `FileUtils.touch({filename})` instead of `File.open` in append mode with empty block.");
        let replacement = format!("FileUtils.touch({filename})");
        let l = node.location();
        let (start, end) = (l.start_offset(), l.end_offset());
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
