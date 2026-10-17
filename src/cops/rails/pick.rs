use crate::prelude::*;

pub const NAME: &str = "Rails/Pick";
pub static COP: Pick = Pick;
pub struct Pick;

impl Cop for Pick {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        6.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call (call _ :pluck ...) :first)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "first" || !send_args(&call).is_empty() {
            return;
        }
        let Some(recv) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&recv) != "pluck" || recv.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let (Some(rsel), Some(sel)) = (recv.message_loc(), call.message_loc()) else { return };
        let (s, e) = (rsel.start_offset(), sel.end_offset());
        let args: Vec<&str> = send_args(&recv).iter().map(|a| node_text(a)).collect();
        let msg = format!("Prefer `pick({})` over `{}`.", args.join(", "), cx.src.slice(s, e));
        let recv_end = recv.location().end_offset();
        let (rs, re) = (rsel.start_offset(), rsel.end_offset());
        cx.add_fix(s, e, msg, |c| {
            c.remove(recv_end, e);
            c.replace(rs, re, "pick");
        });
    }
}
