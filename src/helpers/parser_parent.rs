//! The parser-gem parent of a node, and small parser-pattern predicates.

use crate::prelude::*;

/// The parser-gem parent of the node whose Prism ancestors are `ancestors` (Prism-only wrappers
/// and a call owning the block we are inside skipped).
pub fn parser_parent<'a, 'pr>(ancestors: &'a [Node<'pr>]) -> Option<&'a Node<'pr>> {
    (0..ancestors.len()).rev().find_map(|i| {
        let f = frame(&ancestors[i]);
        let owns_block = f == Frame::Call && ancestors.get(i + 1).is_some_and(|n| frame(n) == Frame::Block);
        (f != Frame::Transparent && !owns_block).then(|| &ancestors[i])
    })
}

/// `(const {nil? cbase} :Name)`
pub fn is_top_const(node: &Node, name: &str) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        return c.name().as_slice() == name.as_bytes();
    }
    node.as_constant_path_node()
        .is_some_and(|p| p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == name.as_bytes()))
}

/// rubocop-ast `operator_keyword?` (`and` / `or` nodes, either spelling).
pub fn is_operator_keyword(node: &Node) -> bool {
    matches!(kind::of(node), kind::AND_NODE | kind::OR_NODE)
}

/// A ternary `a ? b : c`.
pub fn is_ternary(node: &Node) -> bool {
    node.as_if_node().is_some_and(|i| i.if_keyword_loc().is_none())
}

/// Index of the parser-gem parent of `ancestors[i]` (of the visited node itself when
/// `i == ancestors.len()`). A BlockNode's parser parent is its owning call's parent.
pub fn parser_parent_index(ancestors: &[Node], i: usize) -> Option<usize> {
    let end = if i < ancestors.len() && i > 0 && kind::of(&ancestors[i]) == kind::BLOCK_NODE { i - 1 } else { i };
    let p = parser_parent(&ancestors[..end])?;
    (0..end).rev().find(|&k| std::ptr::eq(&ancestors[k], p))
}

/// `node.parent&.defined_module`: the parent is a class/module (name, superclass or single body
/// statement), or the casgn `Foo::Bar = Class.new` / `Module.new` (call or block) whose scope we are.
pub fn defines_module(ancestors: &[Node]) -> bool {
    let n = ancestors.len();
    if n >= 2
        && kind::of(&ancestors[n - 1]) == kind::CONSTANT_PATH_NODE
        && let Some(w) = ancestors[n - 2].as_constant_path_write_node()
    {
        return w.value().as_call_node().is_some_and(|c| {
            c.name().as_slice() == b"new"
                && c.receiver().is_some_and(|r| is_top_const(&r, "Class") || is_top_const(&r, "Module"))
        });
    }
    parser_parent(ancestors).is_some_and(|p| matches!(p, Node::ClassNode { .. } | Node::ModuleNode { .. }))
}

/// rubocop-ast `logical_operator?` on an `and` / `or` node: spelled `&&` / `||`.
pub fn is_logical_operator(node: &Node) -> bool {
    let op = node.as_and_node().map(|n| n.operator_loc()).or_else(|| node.as_or_node().map(|n| n.operator_loc()));
    op.is_some_and(|l| matches!(l.as_slice(), b"&&" | b"||"))
}

/// `(parent.operator_keyword? && parent.logical_operator?) || (parent.if_type? && parent.ternary?)`:
/// a `return` there needs parentheses once it takes an argument.
pub fn return_needs_parentheses(parent: &Node) -> bool {
    is_logical_operator(parent) || is_ternary(parent)
}
