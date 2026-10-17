use crate::prelude::*;

pub const NAME: &str = "RSpec/MultipleDescribes";
pub static COP: MultipleDescribes = MultipleDescribes;
pub struct MultipleDescribes;

impl Cop for MultipleDescribes {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let groups: Vec<Node> =
            helpers::rspec::top_level_groups(&l, node).into_iter().filter(|g| helpers::rspec::example_group(&l, g)).collect();
        if groups.len() < 2 {
            return;
        }
        let (s, e) = send_range(&groups[0].as_call_node().unwrap());
        cx.add(s, e, "Do not use multiple top-level example groups - try to nest them.");
    }
}
