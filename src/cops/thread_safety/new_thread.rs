use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "ThreadSafety/NewThread";
pub static COP: NewThread = NewThread;
pub struct NewThread;

impl Cop for NewThread {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call (const {nil? cbase} :Thread) {:new :fork :start} ...)
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "new" | "fork" | "start") || !call.receiver().is_some_and(|r| is_top_const(&r, "Thread")) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Avoid starting new threads.");
    }
}
