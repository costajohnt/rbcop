//! RuboCop's GemspecHelp mixin.

use crate::prelude::*;
use ruby_prism::CallNode;

/// `Gem::Specification.new` / `::Gem::Specification.new` without arguments.
fn is_spec_new(call: &CallNode) -> bool {
    if is_csend(call) || method_name(call) != "new" || call.arguments().is_some() {
        return false;
    }
    let Some(spec) = call.receiver().and_then(|r| r.as_constant_path_node()) else { return false };
    if spec.name().map(|n| n.as_slice()) != Some(b"Specification") {
        return false;
    }
    match spec.parent() {
        Some(p) => {
            p.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Gem")
                || p.as_constant_path_node()
                    .is_some_and(|c| c.parent().is_none() && c.name().map(|n| n.as_slice()) == Some(b"Gem"))
        }
        None => false,
    }
}

/// `gem_specification?`: `Gem::Specification.new do |x| ... end` with exactly one plain block
/// parameter; returns its name.
pub fn spec_block_var<'pr>(call: &CallNode<'pr>) -> Option<&'pr [u8]> {
    if !is_spec_new(call) {
        return None;
    }
    let params = call.block()?.as_block_node()?.parameters()?.as_block_parameters_node()?;
    if params.locals().iter().next().is_some() {
        return None;
    }
    let p = params.parameters()?;
    let only_required = p.optionals().iter().next().is_none()
        && crate::ast::parser_rest(p.rest()).is_none()
        && p.posts().iter().next().is_none()
        && p.keywords().iter().next().is_none()
        && p.keyword_rest().is_none()
        && p.block().is_none();
    let mut req = p.requireds().iter();
    let (Some(first), None) = (req.next(), req.next()) else { return None };
    if !only_required {
        return None;
    }
    first.as_required_parameter_node().map(|r| r.name().as_slice())
}

/// `enclosing_specification`: start offset of the nearest enclosing gem specification block.
pub fn enclosing_spec(ancestors: &[Node]) -> Option<usize> {
    ancestors.iter().rev().find_map(|a| {
        a.as_call_node().filter(|c| spec_block_var(c).is_some()).map(|c| c.location().start_offset())
    })
}

/// `(lvar name)` receiver name (`it` in an `it` block reads as `(lvar :it)` in parser).
pub fn lvar_name<'pr>(node: &Node<'pr>) -> Option<&'pr [u8]> {
    if let Some(l) = node.as_local_variable_read_node() {
        return Some(l.name().as_slice());
    }
    node.as_it_local_variable_read_node().map(|_| b"it".as_slice())
}
