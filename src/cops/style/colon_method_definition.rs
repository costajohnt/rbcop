use crate::prelude::*;

pub const NAME: &str = "Style/ColonMethodDefinition";
pub static COP: ColonMethodDefinition = ColonMethodDefinition;
pub struct ColonMethodDefinition;

impl Cop for ColonMethodDefinition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(op) = node.as_def_node().unwrap().operator_loc().filter(|o| o.as_slice() == b"::") else { return };
        let (s, e) = (op.start_offset(), op.end_offset());
        cx.add_fix(s, e, "Do not use `::` for defining class methods.", |c| c.replace(s, e, "."));
    }
}
