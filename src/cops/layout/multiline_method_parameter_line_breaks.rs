use crate::helpers::element_line_breaks as elb;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineMethodParameterLineBreaks";
pub static COP: MultilineMethodParameterLineBreaks = MultilineMethodParameterLineBreaks;
pub struct MultilineMethodParameterLineBreaks;

const MSG: &str = "Each parameter in a multi-line method definition must start on a separate line.";

impl Cop for MultilineMethodParameterLineBreaks {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(params) = node.as_def_node().unwrap().parameters() else { return };
        let children = elb::ranges(parameter_nodes(&params));
        elb::check_line_breaks(cx, &children, elb::ignore_last(cx), MSG);
    }
}
