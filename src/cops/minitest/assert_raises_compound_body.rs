use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertRaisesCompoundBody";
pub static COP: AssertRaisesCompoundBody = AssertRaisesCompoundBody;
pub struct AssertRaisesCompoundBody;

impl Cop for AssertRaisesCompoundBody {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = block_call(node) else { return };
        if method_name(&call) != "assert_raises" {
            return;
        }
        let block = call.block().unwrap().as_block_node().unwrap();
        if !block.body().is_some_and(|b| is_begin(&b)) {
            return;
        }
        let (s, e) = range(node);
        cx.add(s, e, "Reduce `assert_raises` block body to contain only the raising code.");
    }
}
