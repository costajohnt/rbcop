use crate::prelude::*;

pub const NAME: &str = "Performance/BindCall";
pub static COP: BindCall = BindCall;
pub struct BindCall;

impl Cop for BindCall {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send $(send _ :bind $(...)) :call $...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "call" {
            return;
        }
        let Some(bind) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if is_csend(&bind) || method_name(&bind) != "bind" || bind.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let bind_args = send_args(&bind);
        let [bind_arg] = bind_args.as_slice() else { return };
        let call_args: Vec<&str> = send_args(&call).iter().map(|a| node_text(a)).collect();
        let call_args = call_args.join(", ");
        let bind_arg = node_text(bind_arg);
        let comma = if call_args.is_empty() { "" } else { ", " };
        let msg = format!("Use `bind_call({bind_arg}{comma}{call_args})` instead of `bind({bind_arg}).call({call_args})`.");
        let start = bind.message_loc().unwrap().start_offset();
        let end = send_range(&call).1;
        let replacement = format!("bind_call({bind_arg}{comma}{call_args})");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
