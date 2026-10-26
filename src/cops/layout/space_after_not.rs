use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceAfterNot";
pub static COP: SpaceAfterNot = SpaceAfterNot;
pub struct SpaceAfterNot;

impl Cop for SpaceAfterNot {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // prefix_bang?: `!x`, not `not x`
        if method_name(&call) != "!" || call.message_loc().is_none_or(|l| l.as_slice() != b"!") {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        // unary_tilde?(receiver): `! ~x` joined would read as `!~`.
        if recv.as_call_node().is_some_and(|r| {
            !crate::ast::is_csend(&r) && helpers::node_types::is_unary_operation(&r) && method_name(&r) == "~"
        }) {
            return;
        }
        let (start, end) = send_range(&call);
        let (sel_end, recv_start) = (call.message_loc().unwrap().end_offset(), recv.location().start_offset());
        if recv_start > start + 1 {
            cx.add_fix(start, end, "Do not leave space between `!` and its argument.", |c| c.remove(sel_end, recv_start));
        }
    }
}
