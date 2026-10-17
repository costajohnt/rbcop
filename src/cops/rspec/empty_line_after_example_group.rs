use crate::prelude::*;

pub const NAME: &str = "RSpec/EmptyLineAfterExampleGroup";
pub static COP: EmptyLineAfterExampleGroup = EmptyLineAfterExampleGroup;
pub struct EmptyLineAfterExampleGroup;

impl Cop for EmptyLineAfterExampleGroup {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_block (plain blocks only): spec_group?
        let l = helpers::rspec::lang(cx);
        if helpers::rspec::plain_block_call(node).is_none() || !helpers::rspec::spec_group(&l, node) {
            return;
        }
        let name = method_name(&node.as_call_node().unwrap()).to_string();
        helpers::rspec::separating_line_offense(cx, node, || format!("Add an empty line after `{name}`."));
    }
}
