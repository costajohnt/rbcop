use crate::prelude::*;

pub const NAME: &str = "Style/StringChars";
pub static COP: StringChars = StringChars;
pub struct StringChars;

impl Cop for StringChars {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "split" {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        if !matches!(node_text(arg), "//" | "''" | "\"\"") {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (start, end) = (sel.start_offset(), send_range(&call).1);
        let msg = format!("Use `chars` instead of `{}`.", &cx.src.text[start..end]);
        cx.add_fix(start, end, msg, |c| c.replace(start, end, "chars"));
    }
}
