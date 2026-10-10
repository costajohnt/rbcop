use crate::helpers::element_line_breaks as elb;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineArrayLineBreaks";
pub static COP: MultilineArrayLineBreaks = MultilineArrayLineBreaks;
pub struct MultilineArrayLineBreaks;

const MSG: &str = "Each item in a multi-line array must start on a separate line.";

impl Cop for MultilineArrayLineBreaks {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE, kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // Parser wraps a `rescue A, B` exception list in an array.
        let children = match node.as_array_node() {
            Some(a) => elb::ranges(a.elements().iter()),
            None => elb::ranges(node.as_rescue_node().unwrap().exceptions().iter()),
        };
        elb::check_line_breaks(cx, &children, elb::ignore_last(cx), MSG);
    }
}
