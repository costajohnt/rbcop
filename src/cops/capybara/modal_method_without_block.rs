use crate::prelude::*;

pub const NAME: &str = "Capybara/ModalMethodWithoutBlock";
pub static COP: ModalMethodWithoutBlock = ModalMethodWithoutBlock;
pub struct ModalMethodWithoutBlock;

impl Cop for ModalMethodWithoutBlock {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "accept_alert" | "accept_confirm" | "accept_prompt" | "dismiss_confirm" | "dismiss_prompt") {
            return;
        }
        // block_literal? / block_argument?, or a forwarded `...` last argument
        if call.block().is_some() || call_args(&call).last().is_some_and(|a| a.as_forwarding_arguments_node().is_some()) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, format!("Call `{name}` with a block."));
    }
}
