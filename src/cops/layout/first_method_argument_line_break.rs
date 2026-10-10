use crate::helpers::element_line_breaks as elb;
use crate::prelude::*;

pub const NAME: &str = "Layout/FirstMethodArgumentLineBreak";
pub static COP: FirstMethodArgumentLineBreak = FirstMethodArgumentLineBreak;
pub struct FirstMethodArgumentLineBreak;

const MSG: &str = "Add a line break before the first argument of a multi-line method argument list.";

impl Cop for FirstMethodArgumentLineBreak {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (name, args) = match node.as_call_node() {
            Some(call) => (method_name(&call), send_args(&call)),
            None => ("super", elb::super_args(&node.as_super_node().unwrap())),
        };
        if helpers::allowed::allowed_method(cx.cfg, name) {
            return;
        }
        let children = elb::flattened_args(args);
        elb::check_method_line_break(cx, node.location().start_offset(), &children, elb::ignore_last(cx), MSG);
    }
}
