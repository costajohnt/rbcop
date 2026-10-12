use crate::prelude::*;

pub const NAME: &str = "Rails/PluckId";
pub static COP: PluckId = PluckId;
pub struct PluckId;

impl Cop for PluckId {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call _ :pluck {(sym :id) (send nil? :primary_key)})
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "pluck" {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        let id = helpers::rails::sym_value(arg).as_deref() == Some("id")
            || helpers::rails::send_node(arg)
                .is_some_and(|c| helpers::rails::is_command(&c, "primary_key") && send_args(&c).is_empty());
        let range = send_range(&call);
        if !id || helpers::rails::in_where(cx.ancestors, range) {
            return;
        }
        let s = call.message_loc().unwrap().start_offset();
        let e = range.1;
        let msg = format!("Use `ids` instead of `{}`.", cx.src.slice(s, e));
        cx.add_fix(s, e, msg, |c| c.replace(s, e, "ids"));
    }
}
