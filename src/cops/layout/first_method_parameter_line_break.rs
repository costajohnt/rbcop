use crate::helpers::element_line_breaks as elb;
use crate::prelude::*;

pub const NAME: &str = "Layout/FirstMethodParameterLineBreak";
pub static COP: FirstMethodParameterLineBreak = FirstMethodParameterLineBreak;
pub struct FirstMethodParameterLineBreak;

const MSG: &str = "Add a line break before the first parameter of a multi-line method parameter list.";

impl Cop for FirstMethodParameterLineBreak {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(params) = node.as_def_node().unwrap().parameters() else { return };
        let children = elb::ranges(parameter_nodes(&params));
        elb::check_method_line_break(cx, node.location().start_offset(), &children, elb::ignore_last(cx), MSG);
    }
}
