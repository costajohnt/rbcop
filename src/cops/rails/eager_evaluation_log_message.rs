use crate::prelude::*;

pub const NAME: &str = "Rails/EagerEvaluationLogMessage";
pub static COP: EagerEvaluationLogMessage = EagerEvaluationLogMessage;
pub struct EagerEvaluationLogMessage;

impl Cop for EagerEvaluationLogMessage {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "debug" || call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let Some(logger) = call.receiver().and_then(|r| helpers::rails::send_node(&r)) else { return };
        if method_name(&logger) != "logger"
            || !send_args(&logger).is_empty()
            || !logger.receiver().is_some_and(|r| helpers::rails::is_const(&r, "Rails"))
        {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        if !is_parser_dstr(arg) {
            return;
        }
        let sel_end = call.message_loc().unwrap().end_offset();
        let (_, e) = send_range(&call);
        let (s, replacement) = if call.opening_loc().is_some() {
            (sel_end, format!(" {{ {} }}", node_text(arg)))
        } else {
            (sel_end + 1, format!("{{ {} }}", node_text(arg)))
        };
        cx.add_fix(s, e, "Pass a block to `Rails.logger.debug`.", |c| c.replace(s, e, &replacement));
    }
}
