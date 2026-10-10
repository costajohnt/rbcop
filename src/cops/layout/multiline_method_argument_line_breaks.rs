use crate::helpers::element_line_breaks as elb;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineMethodArgumentLineBreaks";
pub static COP: MultilineMethodArgumentLineBreaks = MultilineMethodArgumentLineBreaks;
pub struct MultilineMethodArgumentLineBreaks;

const MSG: &str = "Each argument in a multi-line method call must start on a separate line.";

impl Cop for MultilineMethodArgumentLineBreaks {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.name().as_slice() == b"[]=" {
            return;
        }
        let children = elb::flattened_args(send_args(&call));
        elb::check_line_breaks(cx, &children, elb::ignore_last(cx), MSG);
    }
}
