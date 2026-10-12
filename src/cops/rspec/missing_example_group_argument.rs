use crate::prelude::*;

pub const NAME: &str = "RSpec/MissingExampleGroupArgument";
pub static COP: MissingExampleGroupArgument = MissingExampleGroupArgument;
pub struct MissingExampleGroupArgument;

impl Cop for MissingExampleGroupArgument {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if !helpers::rspec::example_group(&l, node) || !send_args(&call).is_empty() {
            return;
        }
        let (s, e) = helpers::rspec::node_range(node);
        cx.add(s, e, format!("The first argument to `{}` should not be empty.", method_name(&call)));
    }
}
