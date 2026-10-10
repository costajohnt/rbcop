use crate::helpers::element_line_breaks as elb;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineHashKeyLineBreaks";
pub static COP: MultilineHashKeyLineBreaks = MultilineHashKeyLineBreaks;
pub struct MultilineHashKeyLineBreaks;

const MSG: &str = "Each key in a multi-line hash must start on a separate line.";

impl Cop for MultilineHashKeyLineBreaks {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let children = elb::ranges(node.as_hash_node().unwrap().elements().iter());
        elb::check_line_breaks(cx, &children, elb::ignore_last(cx), MSG);
    }
}
