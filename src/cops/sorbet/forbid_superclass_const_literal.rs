use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidSuperclassConstLiteral";
pub static COP: ForbidSuperclassConstLiteral = ForbidSuperclassConstLiteral;
pub struct ForbidSuperclassConstLiteral;

impl Cop for ForbidSuperclassConstLiteral {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (class (const ...) $(send ...) ...): a send, not a csend or a block.
        let Some(sup) = node.as_class_node().unwrap().superclass() else { return };
        let Some(call) = sup.as_call_node() else { return };
        if is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let (s, e) = helpers::node_types::range(&sup);
        cx.add(s, e, "Superclasses must only contain constant literals");
    }
}
