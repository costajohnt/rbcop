use crate::prelude::*;

pub const NAME: &str = "Rails/Exit";
pub static COP: Exit = Exit;
pub struct Exit;

impl Cop for Exit {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        if !matches!(name, "exit" | "exit!" | "abort") || send_args(&call).len() > 1 {
            return;
        }
        // `_a, receiver_node_class, _c = *receiver_node`: a const's name, or a send's method.
        let ok = match call.receiver() {
            None => true,
            Some(r) => {
                let second = if let Some(c) = r.as_call_node() {
                    Some(method_name(&c).to_string())
                } else if let Some(c) = r.as_constant_read_node() {
                    Some(String::from_utf8_lossy(c.name().as_slice()).into_owned())
                } else {
                    r.as_constant_path_node().and_then(|p| p.name()).map(|n| String::from_utf8_lossy(n.as_slice()).into_owned())
                };
                matches!(second.as_deref(), Some("Kernel" | "Process"))
            }
        };
        if !ok {
            return;
        }
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), format!("Do not use `{name}` in Rails applications."));
    }
}
