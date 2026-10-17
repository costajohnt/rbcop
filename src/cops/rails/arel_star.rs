use crate::prelude::*;

pub const NAME: &str = "Rails/ArelStar";
pub static COP: ArelStar = ArelStar;
pub struct ArelStar;

impl Cop for ArelStar {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send {const (send _ :arel_table)} :[] $(str "*"))
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "[]" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let recv_ok = recv.as_constant_read_node().is_some()
            || recv.as_constant_path_node().is_some()
            || helpers::rails::send_node(&recv)
                .is_some_and(|r| method_name(&r) == "arel_table" && send_args(&r).is_empty());
        if !recv_ok {
            return;
        }
        let args = send_args(&call);
        let [star] = args.as_slice() else { return };
        if helpers::rails::str_value(star).as_deref() != Some("*") {
            return;
        }
        let (s, e) = (star.location().start_offset(), star.location().end_offset());
        cx.add_fix(s, e, "Use `Arel.star` instead of `\"*\"` for expanded column lists.", |c| {
            c.replace(s, e, "Arel.star")
        });
    }
}
