use crate::prelude::*;

pub const NAME: &str = "RSpec/BeEq";
pub static COP: BeEq = BeEq;
pub struct BeEq;

impl Cop for BeEq {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :eq {boolean nil})
        let Some(call) = helpers::rspec::nil_send(node, "eq") else { return };
        let [arg] = &send_args(&call)[..] else { return };
        if !matches!(kind::of(arg), kind::TRUE_NODE | kind::FALSE_NODE | kind::NIL_NODE) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Prefer `be` over `eq`.", |c| c.replace(s, e, "be"));
    }
}
