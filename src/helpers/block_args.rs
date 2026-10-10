//! Parser-gem `args` children of a block's parameter list (`|a, (b, c); d|` or a lambda's `(...)`).

use crate::node_kind as kind;
use ruby_prism::{BlockParametersNode, Node};

/// Children of the parser `args` node, in source order: arg/optarg/restarg/kwarg/kwoptarg/
/// kwrestarg/blockarg/mlhs/shadowarg. Prism's ImplicitRestNode (`|a,|`) has no parser node.
pub fn children<'pr>(params: &BlockParametersNode<'pr>) -> Vec<Node<'pr>> {
    let mut out: Vec<Node<'pr>> = Vec::new();
    if let Some(p) = params.parameters() {
        out.extend(p.requireds().iter());
        out.extend(p.optionals().iter());
        out.extend(crate::ast::parser_rest(p.rest()));
        out.extend(p.posts().iter());
        out.extend(p.keywords().iter());
        out.extend(p.keyword_rest());
        out.extend(p.block().map(|b| b.as_node()));
    }
    out.extend(params.locals().iter());
    finish(out)
}

/// Children of a parser `mlhs` (Prism MultiTargetNode).
pub fn mlhs_children<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    let Some(m) = node.as_multi_target_node() else { return Vec::new() };
    let mut out: Vec<Node<'pr>> = m.lefts().iter().collect();
    out.extend(crate::ast::parser_rest(m.rest()));
    out.extend(m.rights().iter());
    finish(out)
}

fn finish(mut out: Vec<Node>) -> Vec<Node> {
    out.sort_by_key(|n| n.location().start_offset());
    out
}

/// The parameter list has at least one parser `args` child (`node.arguments?`).
pub fn any(params: &BlockParametersNode) -> bool {
    !children(params).is_empty()
}

/// Number of plain required params (parser `:arg`) anywhere in the list, nested mlhs included.
pub fn count_required(params: &BlockParametersNode) -> usize {
    fn walk(nodes: Vec<Node>) -> usize {
        nodes
            .into_iter()
            .map(|n| match kind::of(&n) {
                kind::REQUIRED_PARAMETER_NODE => 1,
                kind::MULTI_TARGET_NODE => walk(mlhs_children(&n)),
                _ => 0,
            })
            .sum()
    }
    walk(children(params))
}

/// Block params match `(args (arg _))`, or `(args)` when `allow_empty`; a numblock only with `_1`,
/// an itblock always (Lint/RedundantWithIndex, Lint/RedundantWithObject).
pub fn single_arg_or_numbered(block: &ruby_prism::BlockNode, allow_empty: bool) -> bool {
    let Some(params) = block.parameters() else { return allow_empty };
    if let Some(n) = params.as_numbered_parameters_node() {
        return n.maximum() == 1;
    }
    if params.as_it_parameters_node().is_some() {
        return true;
    }
    let Some(bp) = params.as_block_parameters_node() else { return false };
    let children = children(&bp);
    match children.as_slice() {
        [] => allow_empty,
        [one] => kind::of(one) == kind::REQUIRED_PARAMETER_NODE,
        _ => false,
    }
}
