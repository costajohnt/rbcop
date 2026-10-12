use crate::prelude::*;

pub const NAME: &str = "Performance/ConcurrentMonotonicTime";
pub static COP: ConcurrentMonotonicTime = ConcurrentMonotonicTime;
pub struct ConcurrentMonotonicTime;

impl Cop for ConcurrentMonotonicTime {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? cbase} :Concurrent) :monotonic_time ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "monotonic_time" {
            return;
        }
        if !call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Concurrent")) {
            return;
        }
        let unit = send_args(&call).first().map(|a| format!(", {}", node_text(a))).unwrap_or_default();
        let prefer = format!("Process.clock_gettime(Process::CLOCK_MONOTONIC{unit})");
        let (start, end) = send_range(&call);
        let msg = format!("Use `{prefer}` instead of `{}`.", cx.src.slice(start, end));
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &prefer));
    }
}
