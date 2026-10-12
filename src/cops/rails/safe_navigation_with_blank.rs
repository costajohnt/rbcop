use crate::prelude::*;

pub const NAME: &str = "Rails/SafeNavigationWithBlank";
pub static COP: SafeNavigationWithBlank = SafeNavigationWithBlank;
pub struct SafeNavigationWithBlank;

impl Cop for SafeNavigationWithBlank {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (if $(csend ... :blank?) ...)
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let pred = match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().predicate(),
            _ => node.as_unless_node().unwrap().predicate(),
        };
        let Some(csend) = pred.as_call_node().filter(|c| is_csend(c) && method_name(c) == "blank?") else { return };
        if csend.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let dot = csend.call_operator_loc().unwrap();
        let (ds, de) = (dot.start_offset(), dot.end_offset());
        let s = node.location().start_offset();
        let e = helpers::if_node::parser_end(node);
        cx.add_fix(s, e, "Avoid calling `blank?` with the safe navigation operator in conditionals.", |c| {
            c.replace(ds, de, ".")
        });
    }
}
