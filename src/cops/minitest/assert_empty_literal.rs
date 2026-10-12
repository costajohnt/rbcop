use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertEmptyLiteral";
pub static COP: AssertEmptyLiteral = AssertEmptyLiteral;
pub struct AssertEmptyLiteral;

impl Cop for AssertEmptyLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(args) = receiverless(&call, &["assert_equal"]) else { return };
        if args.len() < 2 {
            return;
        }
        let empty = match &args[0] {
            Node::ArrayNode { .. } => args[0].as_array_node().unwrap().elements().iter().next().is_none(),
            Node::HashNode { .. } => {
                !args[0].as_hash_node().unwrap().elements().iter().any(|e| e.as_assoc_node().is_some())
            }
            _ => return,
        };
        if !empty {
            return;
        }
        let rest: Vec<&str> = args[1..].iter().map(|a| node_text(a)).collect();
        let msg = format!("Prefer using `assert_empty({})`.", rest.join(", "));
        let object = rest[0];
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let r = first_and_second(&args);
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, msg, |c| {
            c.replace(ss, se, "assert_empty");
            c.replace(r.0, r.1, object);
        });
    }
}
