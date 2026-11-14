use crate::helpers::parser_parent::is_top_const;
use crate::helpers::sorbet_rbs_assertion::descendants;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTypeAliasedShapes";
pub static COP: ForbidTypeAliasedShapes = ForbidTypeAliasedShapes;
pub struct ForbidTypeAliasedShapes;

impl Cop for ForbidTypeAliasedShapes {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send (const {nil? cbase} :T) :type_alias) (args) `hash)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "type_alias"
            || is_csend(&call)
            || call.arguments().is_some()
            || !call.receiver().is_some_and(|r| is_top_const(&r, "T"))
        {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let no_params = match block.parameters() {
            None => true,
            Some(p) => p.as_block_parameters_node().is_some_and(|p| p.parameters().is_none() && p.locals().iter().next().is_none()),
        };
        let Some(body) = block.body() else { return };
        if no_params && descendants(&body).iter().any(|n| matches!(n, Node::HashNode { .. } | Node::KeywordHashNode { .. })) {
            let (s, e) = helpers::node_types::range(node);
            cx.add(s, e, "Type aliases shouldn't contain shapes because of significant performance overhead");
        }
    }
}
