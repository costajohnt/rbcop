use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/UnreachableAssertion";
pub static COP: UnreachableAssertion = UnreachableAssertion;
pub struct UnreachableAssertion;

impl Cop for UnreachableAssertion {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = block_call(node) else { return };
        if method_name(&call) != "assert_raises" {
            return;
        }
        let block = call.block().unwrap().as_block_node().unwrap();
        let Some(body) = block.body() else { return };
        if !is_begin(&body) {
            return;
        }
        let last = body.as_statements_node().unwrap().body().iter().last().unwrap();
        let Some(send) = as_send(&last) else { return };
        if !send_is_assertion(&send) {
            return;
        }
        let (s, e) = send_range(&send);
        cx.add(s, e, format!("Unreachable `{}` detected.", method_name(&send)));
    }
}
