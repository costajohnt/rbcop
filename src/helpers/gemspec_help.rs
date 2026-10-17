//! RuboCop's GemspecHelp mixin.

use crate::prelude::*;
use ruby_prism::CallNode;

/// `Gem::Specification.new` / `::Gem::Specification.new` without arguments.
pub fn is_spec_new(call: &CallNode) -> bool {
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

/// `(lvar name)` receiver name (`it` in an `it` block reads as `(lvar :it)` in parser). Below
/// Ruby 2.7 a `_1`..`_9` read is `(send nil :_1)` (`node_types::is_legacy_numbered_read`).
pub fn lvar_name<'pr>(node: &Node<'pr>, numbered_sends: &[usize]) -> Option<&'pr [u8]> {
    if let Some(l) = node.as_local_variable_read_node() {
        let send = numbered_sends.binary_search(&l.location().start_offset()).is_ok();
        return (!send).then_some(l.name().as_slice());
    }
    node.as_it_local_variable_read_node().map(|_| b"it".as_slice())
}

struct FirstSpec<'pr>(Option<&'pr [u8]>);

impl<'pr> ruby_prism::Visit<'pr> for FirstSpec<'pr> {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if self.0.is_none() {
            self.0 = spec_block_var(node);
            ruby_prism::visit_call_node(self, node);
        }
    }
}

/// `match_block_variable_name?`'s target: the block variable of the first
/// `Gem::Specification.new do |x|` in the file (pre-order), nil without one.
pub fn first_spec_var<'pr>(program: &Node<'pr>) -> Option<&'pr [u8]> {
    let mut v = FirstSpec(None);
    ruby_prism::Visit::visit(&mut v, program);
    v.0
}
