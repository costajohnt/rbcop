use crate::prelude::*;

pub const NAME: &str = "RSpec/MissingExpectationTargetMethod";
pub static COP: MissingExpectationTargetMethod = MissingExpectationTargetMethod;
pub struct MissingExpectationTargetMethod;

/// expect?: `{(send nil? :expect ...) (send nil? :is_expected)}` on the send part of `call`.
fn expect_send(call: &ruby_prism::CallNode) -> bool {
    !is_csend(call)
        && call.receiver().is_none()
        && match method_name(call) {
            "expect" => true,
            "is_expected" => send_args(call).is_empty(),
            _ => false,
        }
}

impl Cop for MissingExpectationTargetMethod {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !expect_send(&call) {
            return;
        }
        // {#expect? #expect_block?}: a plain send, or a plain parameterless block
        if let Some(b) = helpers::rspec::block_of(&call)
            && (!helpers::rspec::is_plain(&b)
                || b.parameters().is_some_and(|p| {
                    p.as_block_parameters_node()
                        .is_none_or(|bp| bp.parameters().is_some() || bp.locals().iter().next().is_some())
                }))
        {
            return;
        }
        // (send {#expect? #expect_block?} !#Runners.all ...): the parent send with this as receiver
        let Some(parent) = cx.ancestors.last().and_then(|p| p.as_call_node()) else { return };
        if is_csend(&parent) || helpers::rspec::is_runner(method_name(&parent)) {
            return;
        }
        if parent.receiver().is_none_or(|r| helpers::rspec::node_range(&r) != helpers::rspec::node_range(node)) {
            return;
        }
        let Some(sel) = parent.message_loc() else { return };
        cx.add(sel.start_offset(), sel.end_offset(), "Use `.to`, `.not_to` or `.to_not` to set an expectation.");
    }
}
