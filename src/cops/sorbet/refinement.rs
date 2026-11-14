use crate::helpers::parser_parent::parser_parent;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/Refinement";
pub static COP: Refinement = Refinement;
pub struct Refinement;

impl Cop for Refinement {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "refine" | "using") || is_csend(&call) || call.receiver().is_some() {
            return;
        }
        if !send_args(&call).first().is_some_and(|a| matches!(a, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. })) {
            return;
        }
        // `refine`: only a block (any kind) whose parent is the module itself (a single-statement
        // body). At the top level `node.parent.parent` is nil and RuboCop crashes: no offense.
        if name == "refine"
            && (helpers::node_types::literal_block(node).is_none()
                || !parser_parent(cx.ancestors).is_some_and(|p| p.as_module_node().is_some()))
        {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Do not use Ruby Refinements library as it is not supported by Sorbet.");
    }
}
