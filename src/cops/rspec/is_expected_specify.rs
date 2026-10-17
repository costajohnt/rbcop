use crate::prelude::*;

pub const NAME: &str = "RSpec/IsExpectedSpecify";
pub static COP: IsExpectedSpecify = IsExpectedSpecify;
pub struct IsExpectedSpecify;

impl Cop for IsExpectedSpecify {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send _ :specify) _ (send (send _ IS_EXPECTED_METHODS) ...)), single line
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if is_csend(&call) || method_name(&call) != "specify" || !send_args(&call).is_empty() {
            return;
        }
        if !helpers::rspec::single_line(cx.src, node) {
            return;
        }
        let Some(body) = helpers::rspec::parser_body(helpers::rspec::block_body(&call)) else { return };
        let Some(send) = helpers::rspec::send_call(&body).filter(|c| !is_csend(c)) else { return };
        let Some(recv) = send.receiver().and_then(|r| helpers::rspec::send_call(&r)) else { return };
        if is_csend(&recv) || !matches!(method_name(&recv), "is_expected" | "are_expected") || !send_args(&recv).is_empty() {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Use `it` instead of `specify`.", |c| c.replace(s, e, "it"));
    }
}
