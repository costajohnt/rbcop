use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/Focus";
pub static COP: Focus = Focus;
pub struct Focus;

impl Cop for Focus {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(args) = receiverless(&call, &["focus"]) else { return };
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let r = match args.first() {
            None => {
                let (s, e) = send_range(&call);
                helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true)
            }
            Some(a) => (ss, range(a).0),
        };
        cx.add_fix(ss, se, "Remove `focus` from tests.", |c| c.remove(r.0, r.1));
    }
}
