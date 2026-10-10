use crate::prelude::*;

pub const NAME: &str = "Style/ColonMethodCall";
pub static COP: ColonMethodCall = ColonMethodCall;
pub struct ColonMethodCall;

impl Cop for ColonMethodCall {
    fn correctable(&self) -> bool {
        true
    }

    // Parser keeps a `send` inside op_asgn/or_asgn/and_asgn/mlhs where Prism has dedicated call nodes.
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::CALL_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (receiver, dot, name) = if let Some(c) = node.as_call_node() {
            (c.receiver(), c.call_operator_loc(), c.name())
        } else if let Some(c) = node.as_call_operator_write_node() {
            (c.receiver(), c.call_operator_loc(), c.read_name())
        } else if let Some(c) = node.as_call_or_write_node() {
            (c.receiver(), c.call_operator_loc(), c.read_name())
        } else if let Some(c) = node.as_call_and_write_node() {
            (c.receiver(), c.call_operator_loc(), c.read_name())
        } else {
            let c = node.as_call_target_node().unwrap();
            (Some(c.receiver()), Some(c.call_operator_loc()), c.name())
        };
        let (Some(mut receiver), Some(dot)) = (receiver, dot) else { return };
        // camel_case_method?
        if text(&dot) != "::" || name.as_slice().first().is_some_and(u8::is_ascii_uppercase) {
            return;
        }
        // java_interop?: the innermost receiver is `Java`.
        while let Some(r) = receiver.as_call_node().and_then(|c| c.receiver()) {
            receiver = r;
        }
        if receiver.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Java") {
            return;
        }
        let (s, e) = (dot.start_offset(), dot.end_offset());
        cx.add_fix(s, e, "Do not use `::` for method calls.", |c| c.replace(s, e, "."));
    }
}
