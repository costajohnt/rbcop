use crate::prelude::*;

pub const NAME: &str = "Style/RedundantCurrentDirectoryInPath";
pub static COP: RedundantCurrentDirectoryInPath = RedundantCurrentDirectoryInPath;
pub struct RedundantCurrentDirectoryInPath;

/// Length of a leading `\./+` match.
fn redundant_length(content: &[u8]) -> Option<usize> {
    let rest = content.strip_prefix(b".")?;
    let slashes = rest.iter().take_while(|&&b| b == b'/').count();
    (slashes > 0).then_some(1 + slashes)
}

impl Cop for RedundantCurrentDirectoryInPath {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "require_relative" || is_csend(&call) {
            return;
        }
        let Some(arg) = send_args(&call).into_iter().next() else { return };
        let source = node_text(&arg);
        let Some(index) = source.as_bytes().windows(2).position(|w| w == b"./") else { return };
        // leading_path_content: a str's content, or a dstr's first str child.
        let content = if let Some(s) = arg.as_string_node() {
            s.unescaped().to_vec()
        } else if let Some(d) = arg.as_interpolated_string_node() {
            match d.parts().iter().next().and_then(|p| p.as_string_node()) {
                Some(s) => s.unescaped().to_vec(),
                None => return,
            }
        } else {
            return;
        };
        let Some(len) = redundant_length(&content) else { return };
        let start = arg.location().start_offset() + index;
        let end = start + len;
        cx.add_fix(start, end, "Remove the redundant current directory path.", |c| c.remove(start, end));
    }
}
