use crate::prelude::*;

pub const NAME: &str = "RSpec/EmptyLineAfterExample";
pub static COP: EmptyLineAfterExample = EmptyLineAfterExample;
pub struct EmptyLineAfterExample;

impl Cop for EmptyLineAfterExample {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::example(&l, node) {
            return;
        }
        if cx.cfg.bool("AllowConsecutiveOneLiners") == Some(true) && helpers::rspec::single_line(cx.src, node) {
            let next = helpers::rspec::right_sibling(cx.ancestors, node);
            if next.is_some_and(|n| helpers::rspec::example(&l, &n) && helpers::rspec::single_line(cx.src, &n)) {
                return;
            }
        }
        let name = method_name(&node.as_call_node().unwrap()).to_string();
        helpers::rspec::separating_line_offense(cx, node, || format!("Add an empty line after `{name}`."));
    }
}
