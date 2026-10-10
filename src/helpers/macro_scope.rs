//! rubocop-ast `in_macro_scope?` for Prism nodes.

use crate::node_kind as kind;
use ruby_prism::Node;

/// Whether a node with these Prism ancestors (outermost first) is in a macro scope: the root, a
/// class/module/sclass body, or nested only in wrappers (kwbegin, begin, block, `if` branches)
/// that are themselves in a macro scope. For a BlockNode pass the ancestors *without* its call
/// (parser's block wraps the call), e.g. `&ancestors[..len - 1]`.
pub fn in_macro_scope(ancestors: &[Node]) -> bool {
    let n = ancestors.len();
    let Some(parent) = ancestors.last() else { return true };
    match kind::of(parent) {
        kind::PROGRAM_NODE => true,
        kind::STATEMENTS_NODE => {
            let Some(owner) = n.checked_sub(2).map(|i| &ancestors[i]) else { return true };
            match kind::of(owner) {
                kind::PROGRAM_NODE | kind::CLASS_NODE | kind::MODULE_NODE | kind::SINGLETON_CLASS_NODE => true,
                // block body: the block's parser parent is its call's parent.
                kind::BLOCK_NODE => {
                    n < 3 || class_constructor(&ancestors[n - 3]) || in_macro_scope(&ancestors[..n - 3])
                }
                kind::LAMBDA_NODE | kind::IF_NODE | kind::UNLESS_NODE | kind::PARENTHESES_NODE => {
                    in_macro_scope(&ancestors[..n - 2])
                }
                kind::BEGIN_NODE => {
                    let b = owner.as_begin_node().unwrap();
                    b.begin_keyword_loc().is_some()
                        && b.rescue_clause().is_none()
                        && b.ensure_clause().is_none()
                        && b.else_clause().is_none()
                        && in_macro_scope(&ancestors[..n - 2])
                }
                kind::ELSE_NODE => {
                    n >= 3
                        && matches!(kind::of(&ancestors[n - 3]), kind::IF_NODE | kind::UNLESS_NODE)
                        && in_macro_scope(&ancestors[..n - 3])
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// `class_constructor?` send: `Class.new`, `Module.new`, `Struct.new`, `Data.define` (on a
/// top-level constant, `::` allowed).
pub fn class_constructor(node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    let Some(recv) = call.receiver() else { return false };
    let name = match (recv.as_constant_read_node(), recv.as_constant_path_node()) {
        (Some(c), _) => c.name().as_slice(),
        (_, Some(p)) if p.parent().is_none() => match p.name() {
            Some(n) => n.as_slice(),
            None => return false,
        },
        _ => return false,
    };
    match call.name().as_slice() {
        b"new" => matches!(name, b"Class" | b"Module" | b"Struct"),
        b"define" => name == b"Data",
        _ => false,
    }
}
