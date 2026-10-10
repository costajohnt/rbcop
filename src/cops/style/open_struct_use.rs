use crate::prelude::*;

pub const NAME: &str = "Style/OpenStructUse";
pub static COP: OpenStructUse = OpenStructUse;
pub struct OpenStructUse;

const MSG: &str = "Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.";

impl Cop for OpenStructUse {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !helpers::parser_parent::is_top_const(node, "OpenStruct") {
            return;
        }
        let loc = node.location();
        // custom_class_or_module_definition?: the class/module name itself.
        let defined = cx.parent().and_then(|p| match (p.as_class_node(), p.as_module_node()) {
            (Some(c), _) => Some(c.constant_path().location()),
            (_, Some(m)) => Some(m.constant_path().location()),
            _ => None,
        });
        if defined.is_some_and(|d| d.start_offset() == loc.start_offset() && d.end_offset() == loc.end_offset()) {
            return;
        }
        cx.add(loc.start_offset(), loc.end_offset(), MSG);
    }
}
