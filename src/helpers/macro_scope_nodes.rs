//! Deprecated: use `helpers::macro_scope` (`in_macro_scope_of_child`, `class_constructor`). Kept
//! as thin wrappers so branches written against this earlier port still compile.

use ruby_prism::Node;

/// Deprecated: `macro_scope::in_macro_scope_of_child`.
pub fn in_macro_scope(ancestors: &[Node]) -> bool {
    super::macro_scope::in_macro_scope_of_child(ancestors)
}

/// Deprecated: `macro_scope::class_constructor` on a node that is a CallNode.
pub fn class_constructor(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| super::macro_scope::class_constructor(&c))
}
