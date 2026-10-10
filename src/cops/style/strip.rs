use crate::prelude::*;

pub const NAME: &str = "Style/Strip";
pub static COP: Strip = Strip;
pub struct Strip;

impl Cop for Strip {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let other = match method_name(&call) {
            "lstrip" => "rstrip",
            "rstrip" => "lstrip",
            _ => return,
        };
        if !send_args(&call).is_empty() {
            return;
        }
        // (call $(call _ :rstrip) :lstrip): the receiver is a plain call, not a block.
        let Some(first) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&first) != other
            || !send_args(&first).is_empty()
            || first.block().is_some_and(|b| b.as_block_node().is_some())
        {
            return;
        }
        let Some(sel) = first.message_loc() else { return };
        let (start, end) = (sel.start_offset(), send_range(&call).1);
        let msg = format!("Use `strip` instead of `{}`.", &cx.src.text[start..end]);
        cx.add_fix(start, end, msg, |c| c.replace(start, end, "strip"));
    }
}
