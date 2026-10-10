use crate::prelude::*;

pub const NAME: &str = "Lint/SendWithMixinArgument";
pub static COP: SendWithMixinArgument = SendWithMixinArgument;
pub struct SendWithMixinArgument;

fn is_const(node: &Node) -> bool {
    matches!(kind::of(node), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE)
}

impl Cop for SendWithMixinArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !matches!(method_name(&call), "send" | "public_send" | "__send__") {
            return;
        }
        if call.receiver().is_some_and(|r| r.as_self_node().is_none() && !is_const(&r)) {
            return;
        }
        if call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
            return;
        }
        let args = call_args(&call);
        let Some((first, modules)) = args.split_first() else { return };
        let method = if let Some(s) = first.as_symbol_node() {
            s.unescaped().to_vec()
        } else if let Some(s) = first.as_string_node() {
            s.unescaped().to_vec()
        } else {
            return;
        };
        let method = String::from_utf8_lossy(&method).into_owned();
        if !matches!(method.as_str(), "include" | "prepend" | "extend") || modules.is_empty() || !modules.iter().all(is_const) {
            return;
        }
        let names = modules.iter().map(node_text).collect::<Vec<_>>().join(", ");
        let s = call.message_loc().unwrap().start_offset();
        let e = send_range(&call).1;
        let msg = format!("Use `{method} {names}` instead of `{}`.", cx.src.slice(s, e));
        let fix = format!("{method} {names}");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &fix));
    }
}
