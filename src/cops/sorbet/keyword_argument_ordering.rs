use crate::helpers::sorbet_signature::{index_of, signature, siblings};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/KeywordArgumentOrdering";
pub static COP: KeywordArgumentOrdering = KeywordArgumentOrdering;
pub struct KeywordArgumentOrdering;

/// `method_node.arguments` as parser args: a def's parameters, a block's or lambda's (with its
/// `;x` block-locals last). Other statements have no kwoptarg (sends) or no `arguments` (the cop
/// raises).
fn arguments<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    let block_params = |p: Option<Node<'pr>>| -> Vec<Node<'pr>> {
        let Some(b) = p.and_then(|p| p.as_block_parameters_node()) else { return Vec::new() };
        let mut v = b.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default();
        v.extend(b.locals().iter());
        v
    };
    if let Some(d) = node.as_def_node() {
        return d.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default();
    }
    if let Some(l) = node.as_lambda_node() {
        return block_params(l.parameters());
    }
    node.as_call_node().and_then(|c| c.block()).and_then(|b| b.as_block_node()).map_or_else(Vec::new, |b| block_params(b.parameters()))
}

impl Cop for KeywordArgumentOrdering {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if signature(node).is_none() {
            return;
        }
        let Some(sibs) = siblings(cx.ancestors.last()) else { return };
        let Some(method) = index_of(&sibs, helpers::node_types::range(node)).and_then(|i| sibs.get(i + 1)) else { return };
        let mut out_of_kwoptarg = false;
        for param in arguments(method).iter().rev() {
            let k = kind::of(param);
            if !matches!(k, kind::OPTIONAL_KEYWORD_PARAMETER_NODE | kind::BLOCK_PARAMETER_NODE | kind::KEYWORD_REST_PARAMETER_NODE) {
                out_of_kwoptarg = true;
            }
            if k == kind::OPTIONAL_KEYWORD_PARAMETER_NODE && out_of_kwoptarg {
                let (s, e) = helpers::node_types::range(param);
                cx.add(s, e, "Optional keyword arguments must be at the end of the parameter list.");
            }
        }
    }
}
