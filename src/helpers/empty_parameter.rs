//! RuboCop's EmptyParameter mixin: block parameters with delimiters but nothing inside.

use ruby_prism::Node;

/// The range of empty `||` / `()` block parameters (not `empty_and_without_delimiters?`).
pub fn empty_params(params: Option<Node>) -> Option<(usize, usize)> {
    let bp = params?.as_block_parameters_node()?;
    if bp.parameters().is_some() || bp.locals().iter().next().is_some() {
        return None;
    }
    let loc = bp.location();
    Some((loc.start_offset(), loc.end_offset()))
}
