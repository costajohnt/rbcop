use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertTruthy";
pub static COP: AssertTruthy = AssertTruthy;
pub struct AssertTruthy;

impl Cop for AssertTruthy {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(args) = receiverless(&call, &["assert_equal"]) else { return };
        if args.len() < 2 || args[0].as_true_node().is_none() {
            return;
        }
        let actual = node_text(&args[1]);
        let shown = match args.get(2) {
            Some(m) => format!("{actual}, {}", node_text(m)),
            None => actual.to_string(),
        };
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let r = first_and_second(&args);
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, format!("Prefer using `assert({shown})`."), |c| {
            c.replace(ss, se, "assert");
            c.replace(r.0, r.1, actual);
        });
    }
}
