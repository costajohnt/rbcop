use crate::helpers::literal_node::parser_type;
use crate::prelude::*;

pub const NAME: &str = "Style/ImplicitRuntimeError";
pub static COP: ImplicitRuntimeError = ImplicitRuntimeError;
pub struct ImplicitRuntimeError;

impl Cop for ImplicitRuntimeError {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? ${:raise :fail} {str dstr})
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if call.receiver().is_some() || !matches!(method, "raise" | "fail") {
            return;
        }
        if call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
            return;
        }
        let args = call_args(&call);
        if args.len() != 1 || !matches!(parser_type(&args[0]), "str" | "dstr") {
            return;
        }
        let (start, end) = send_range(&call);
        cx.add(start, end, format!("Use `{method}` with an explicit exception class and message, rather than just a message."));
    }
}
