use crate::prelude::*;

pub const NAME: &str = "Style/Not";
pub static COP: Not = Not;
pub struct Not;

impl Cop for Not {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "!" || is_csend(&call) {
            return;
        }
        if let Some(sel) = call.message_loc().filter(|l| text(l) == "not") {
            cx.add(sel.start_offset(), sel.end_offset(), "Use `!` instead of `not`.");
        }
    }
}
