use crate::helpers::element_line_breaks as elb;
use crate::prelude::*;

pub const NAME: &str = "Layout/FirstHashElementLineBreak";
pub static COP: FirstHashElementLineBreak = FirstHashElementLineBreak;
pub struct FirstHashElementLineBreak;

const MSG: &str = "Add a line break before the first element of a multi-line hash.";

impl Cop for FirstHashElementLineBreak {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let children = elb::ranges(node.as_hash_node().unwrap().elements().iter());
        elb::check_children_line_break(cx, node.location().start_offset(), &children, elb::ignore_last(cx), MSG);
    }
}
