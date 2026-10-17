use crate::prelude::*;

pub const NAME: &str = "RSpec/SkipBlockInsideExample";
pub static COP: SkipBlockInsideExample = SkipBlockInsideExample;
pub struct SkipBlockInsideExample;

impl Cop for SkipBlockInsideExample {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rspec::any_block_call(node) else { return };
        if method_name(&call) != "skip" {
            return;
        }
        let l = helpers::rspec::lang(cx);
        if cx.ancestors.iter().any(|a| helpers::rspec::example(&l, a)) {
            let (s, e) = helpers::rspec::node_range(node);
            cx.add(s, e, "Don't pass a block to `skip` inside examples.");
        }
    }
}
