use crate::prelude::*;

pub const NAME: &str = "RSpec/EmptyLineAfterSubject";
pub static COP: EmptyLineAfterSubject = EmptyLineAfterSubject;
pub struct EmptyLineAfterSubject;

impl Cop for EmptyLineAfterSubject {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::subject(&l, node) || !helpers::rspec::inside_example_group(&l, cx.ancestors, node) {
            return;
        }
        let name = method_name(&node.as_call_node().unwrap()).to_string();
        helpers::rspec::separating_line_offense(cx, node, || format!("Add an empty line after `{name}`."));
    }
}
