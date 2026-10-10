use crate::prelude::*;

pub const NAME: &str = "Security/MarshalLoad";
pub static COP: MarshalLoad = MarshalLoad;
pub struct MarshalLoad;

fn is_marshal(node: &Node) -> bool {
    helpers::const_name::const_name(node).as_deref() == Some("Marshal")
}

/// (send (const {nil? cbase} :Marshal) :dump ...)
fn marshal_dump(node: &Node) -> bool {
    node.as_call_node().is_some_and(|d| {
        !is_csend(&d)
            && d.block().is_none_or(|b| b.as_block_argument_node().is_some())
            && method_name(&d) == "dump"
            && d.receiver().is_some_and(|r| is_marshal(&r))
    })
}

impl Cop for MarshalLoad {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if is_csend(&call) || !matches!(method, "load" | "restore") || !call.receiver().is_some_and(|r| is_marshal(&r)) {
            return;
        }
        // ... !(send (const {nil? cbase} :Marshal) :dump ...) _?
        let ok = match send_args(&call).as_slice() {
            [a] | [a, _] => !marshal_dump(a),
            _ => false,
        };
        if ok {
            let sel = call.message_loc().unwrap();
            cx.add(sel.start_offset(), sel.end_offset(), format!("Avoid using `Marshal.{method}`."));
        }
    }
}
