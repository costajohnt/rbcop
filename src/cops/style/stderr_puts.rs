use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/StderrPuts";
pub static COP: StderrPuts = StderrPuts;
pub struct StderrPuts;

impl Cop for StderrPuts {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "puts" || send_args(&call).is_empty() {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let stderr = recv.as_global_variable_read_node().is_some_and(|g| g.name().as_slice() == b"$stderr")
            || is_top_const(&recv, "STDERR");
        let Some(sel) = call.message_loc().filter(|_| stderr) else { return };
        let (start, end) = (node.location().start_offset(), sel.end_offset());
        let msg = format!("Use `warn` instead of `{}.puts` to allow such output to be disabled.", node_text(&recv));
        cx.add_fix(start, end, msg, |c| c.replace(start, end, "warn"));
    }
}
