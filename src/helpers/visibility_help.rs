//! RuboCop's VisibilityHelp mixin (`node_visibility`) and DefNode#non_public?.

use crate::prelude::*;

const SCOPES: &[&str] = &["private", "protected", "public"];

/// `(send nil? VISIBILITY_SCOPES ...)` with `n` arguments (None: at least one); the scope name.
fn scope_call<'pr>(node: &Node<'pr>, n: Option<usize>) -> Option<(ruby_prism::CallNode<'pr>, &'pr str)> {
    let call = node.as_call_node()?;
    let name = method_name(&call);
    let count = send_args(&call).len();
    // A call with a block literal is a parser `block`, not a `send`.
    (call.receiver().is_none()
        && SCOPES.contains(&name)
        && n.map_or(count > 0, |n| count == n)
        && call.block().is_none_or(|b| b.as_block_node().is_none()))
    .then_some((call, name))
}

/// Parser siblings of the visited node (`ancestors` are its Prism ancestors): the statements of a
/// multi-statement parent, and the node's index among them. A single statement has none.
pub fn siblings<'pr>(ancestors: &[Node<'pr>], node: &Node) -> (Vec<Node<'pr>>, usize) {
    let Some(stmts) = ancestors.last().and_then(|p| p.as_statements_node()) else { return (Vec::new(), 0) };
    let body: Vec<Node> = stmts.body().iter().collect();
    if body.len() < 2 {
        return (Vec::new(), 0);
    }
    let start = node.location().start_offset();
    let idx = body.iter().position(|s| s.location().start_offset() == start).unwrap_or(0);
    (body, idx)
}

/// `node_visibility(node)`: `parent` is the node's parser parent (for `private def x`),
/// `siblings[idx]` the node among its parser siblings (empty when it has none).
pub fn node_visibility(node: &Node, parent: Option<&Node>, siblings: &[Node], idx: usize) -> &'static str {
    let scope = |name: &str| SCOPES.iter().copied().find(|s| *s == name).unwrap_or("public");
    if let Some(def) = node.as_def_node().filter(|d| d.receiver().is_none()) {
        // visibility_inline_on_def?(parent): `(send nil? VISIBILITY_SCOPES def)`
        if let Some((call, name)) = parent.and_then(|p| scope_call(p, Some(1)))
            && call_args(&call).first().is_some_and(|a| a.as_def_node().is_some_and(|d| d.receiver().is_none()))
        {
            return scope(name);
        }
        // visibility_inline_on_method_name?: `(send nil? VISIBILITY_SCOPES <(sym name) ...>)` to
        // the right (1.92: any of the arguments, not only a sole one)
        let def_name = def.name().as_slice();
        if !siblings.is_empty() {
            for s in siblings[idx + 1..].iter().rev() {
                if let Some((call, name)) = scope_call(s, None)
                    && call_args(&call)
                        .iter()
                        .any(|a| a.as_symbol_node().is_some_and(|sym| sym.unescaped() == def_name))
                {
                    return scope(name);
                }
            }
        }
    }
    if !siblings.is_empty() {
        for s in siblings[..idx].iter().rev() {
            if let Some((_, name)) = scope_call(s, Some(0)) {
                return scope(name);
            }
        }
    }
    "public"
}

/// DefNode#non_public_modifier?(parent): `(send nil? {:private :protected :private_class_method} (any_def ...))`.
pub fn non_public_modifier(parent: Option<&Node>) -> bool {
    let Some(call) = parent.and_then(|p| p.as_call_node()) else { return false };
    call.receiver().is_none()
        && matches!(method_name(&call), "private" | "protected" | "private_class_method")
        && call.block().is_none_or(|b| b.as_block_node().is_none())
        && send_args(&call).len() == 1
        && call_args(&call).first().is_some_and(|a| a.as_def_node().is_some())
}
