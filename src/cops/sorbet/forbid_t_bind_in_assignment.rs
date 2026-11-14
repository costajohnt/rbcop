use crate::helpers::sorbet_rbs_assertion::{assigns, is_t_call, send_parent};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTBindInAssignment";
pub static COP: ForbidTBindInAssignment = ForbidTBindInAssignment;
pub struct ForbidTBindInAssignment;

impl Cop for ForbidTBindInAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const nil? :T) :bind _ _), the value its parent assigns
        let call = node.as_call_node().unwrap();
        if !is_t_call(&call, "bind", Some(2)) {
            return;
        }
        let range = send_range(&call);
        if !assigns(send_parent(&call, cx.ancestors), range) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (a, b) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(
            range.0,
            range.1,
            "Do not assign the result of `T.bind`; it also changes the type of its first argument.",
            |c| c.replace(a, b, "cast"),
        );
    }
}
