use crate::prelude::*;

pub const NAME: &str = "Style/TopLevelMethodDefinition";
pub static COP: TopLevelMethodDefinition = TopLevelMethodDefinition;
pub struct TopLevelMethodDefinition;

impl Cop for TopLevelMethodDefinition {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // The root, or a statement of a multi-statement root: Prism ancestors are Program, Statements.
        // Or a statement of a root `( ... )` (a parser `begin` that is the root).
        let anc = cx.ancestors;
        let top = anc.len() == 2
            || (anc.len() == 4
                && anc[1].as_statements_node().is_some_and(|s| s.body().iter().count() == 1)
                && anc[2].as_parentheses_node().is_some());
        if !top {
            return;
        }
        if let Some(call) = node.as_call_node() {
            if method_name(&call) != "define_method" || is_csend(&call) {
                return;
            }
            // on_block: `(any_block (send _ :define_method _) ...)`; the send itself is not top level.
            if call.block().is_some_and(|b| b.as_block_node().is_some()) && call_args(&call).len() != 1 {
                return;
            }
        }
        let loc = node.location();
        cx.add(loc.start_offset(), loc.end_offset(), "Do not define methods at the top-level.");
    }
}
