use crate::prelude::*;

pub const NAME: &str = "Performance/StringBytesize";
pub static COP: StringBytesize = StringBytesize;
pub struct StringBytesize;

impl Cop for StringBytesize {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call (call !{nil? int} :bytes) {:size :length :count})
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "size" | "length" | "count") || !send_args(&call).is_empty() {
            return;
        }
        let Some(bytes) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&bytes) != "bytes" || !send_args(&bytes).is_empty() || bytes.block().is_some() {
            return;
        }
        if bytes.receiver().is_none_or(|r| r.as_integer_node().is_some()) {
            return;
        }
        let start = bytes.message_loc().unwrap().start_offset();
        let end = send_range(&call).1;
        cx.add_fix(start, end, "Use `String#bytesize` instead of calculating the size of the bytes array.", |c| {
            c.replace(start, end, "bytesize")
        });
    }
}
