use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RefuteFalse";
pub static COP: RefuteFalse = RefuteFalse;
pub struct RefuteFalse;

impl Cop for RefuteFalse {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(args) = receiverless(&call, &["assert_equal", "assert"]) else { return };
        let equal = method_name(&call) == "assert_equal";
        let (actual, rest) = if equal {
            if args.len() < 2 || args[0].as_false_node().is_none() {
                return;
            }
            (range(&args[1]), 2)
        } else {
            let Some(recv) = args.first().and_then(|a| unary_send(a, "!")) else { return };
            (range(&recv), 1)
        };
        let actual = &cx.src.text[actual.0..actual.1];
        let shown = match args.get(rest) {
            Some(m) => format!("{actual}, {}", node_text(m)),
            None => actual.to_string(),
        };
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let r = if equal { first_and_second(&args) } else { range(&args[0]) };
        let (s, e) = send_range(&call);
        let actual = actual.to_string();
        cx.add_fix(s, e, format!("Prefer using `refute({shown})`."), |c| {
            c.replace(ss, se, "refute");
            c.replace(r.0, r.1, &actual);
        });
    }
}
