use crate::prelude::*;

pub const NAME: &str = "Style/UnpackFirst";
pub static COP: UnpackFirst = UnpackFirst;
pub struct UnpackFirst;

fn int_zero(node: &Node) -> bool {
    int_literal_value(node) == Some(0)
}

impl Cop for UnpackFirst {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.4, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let args = send_args(&call);
        let ok = match method_name(&call) {
            "first" => args.is_empty(),
            "[]" | "slice" | "at" => args.len() == 1 && int_zero(&args[0]),
            _ => false,
        };
        if !ok {
            return;
        }
        // $(call (...) :unpack $(...))
        let Some(unpack) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        let uargs = send_args(&unpack);
        if method_name(&unpack) != "unpack"
            || unpack.receiver().is_none()
            || uargs.len() != 1
            || unpack.block().is_some_and(|b| b.as_block_node().is_some())
        {
            return;
        }
        let Some(sel) = unpack.message_loc() else { return };
        let end = send_range(&call).1;
        let unpack_end = unpack.location().end_offset();
        let msg = format!(
            "Use `unpack1({})` instead of `{}`.",
            node_text(&uargs[0]),
            &cx.src.text[sel.start_offset()..end]
        );
        cx.add_fix(sel.start_offset(), end, msg, |c| {
            c.remove(unpack_end, end);
            c.replace(sel.start_offset(), sel.end_offset(), "unpack1");
        });
    }
}
