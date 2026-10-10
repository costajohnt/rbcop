//! rubocop-ast `in_macro_scope?` / `macro?` / `class_constructor?` on Prism ancestors: the one port
//! (`macro_scope_nodes` is a deprecated shim over it).

use crate::ast::method_name;
use crate::node_kind as kind;
use ruby_prism::{CallNode, Node};
use std::borrow::Borrow;

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// `class_constructor?` on a block's call: `Class.new`, `Module.new`, `Struct.new`, `Data.define`
/// with a global (`X` or `::X`) constant receiver.
pub fn class_constructor(call: &CallNode) -> bool {
    let Some(r) = call.receiver() else { return false };
    let name = match kind::of(&r) {
        kind::CONSTANT_READ_NODE => r.as_constant_read_node().unwrap().name(),
        kind::CONSTANT_PATH_NODE => {
            let p = r.as_constant_path_node().unwrap();
            match (p.parent(), p.name()) {
                (None, Some(n)) => n,
                _ => return false,
            }
        }
        _ => return false,
    };
    matches!((name.as_slice(), method_name(call)), (b"Class" | b"Module" | b"Struct", "new") | (b"Data", "define"))
}

/// `in_macro_scope?` for the node spanning `subject`, given its Prism ancestors (outermost first):
/// the root, or a child of class/module/sclass/class-constructor block, or of a wrapper
/// (`begin`, `kwbegin`, block, if-branch) that is itself in macro scope.
pub fn in_macro_scope<'pr, N: Borrow<Node<'pr>>>(ancestors: &[N], subject: (usize, usize)) -> bool {
    let mut i = ancestors.len();
    let mut subject = subject;
    loop {
        if i == 0 {
            return true;
        }
        let p = ancestors[i - 1].borrow();
        let step = match kind::of(p) {
            kind::PROGRAM_NODE | kind::CLASS_NODE | kind::MODULE_NODE | kind::SINGLETON_CLASS_NODE => return true,
            // single-statement bodies are transparent; several statements make a `begin` wrapper
            kind::STATEMENTS_NODE | kind::PARENTHESES_NODE | kind::LAMBDA_NODE => 1,
            kind::BLOCK_NODE => {
                // the block's parser node is its call (or `super`), one level further up
                if i < 2 {
                    return false;
                }
                if ancestors[i - 2].borrow().as_call_node().is_some_and(|c| class_constructor(&c)) {
                    return true;
                }
                2
            }
            kind::BEGIN_NODE => {
                let b = p.as_begin_node().unwrap();
                // implicit begin, or rescue/ensure in between: not a wrapper
                if b.begin_keyword_loc().is_none() || b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                    return false;
                }
                1
            }
            kind::IF_NODE | kind::UNLESS_NODE => {
                let pred = match p.as_if_node() {
                    Some(n) => n.predicate(),
                    None => p.as_unless_node().unwrap().predicate(),
                };
                if span(&pred) == subject {
                    return false;
                }
                1
            }
            kind::ELSE_NODE => {
                if !i.checked_sub(2).is_some_and(|j| matches!(kind::of(ancestors[j].borrow()), kind::IF_NODE | kind::UNLESS_NODE)) {
                    return false;
                }
                1
            }
            _ => return false,
        };
        i -= step;
        subject = span(ancestors[i].borrow());
    }
}

/// `macro?`: no receiver and in macro scope.
pub fn is_macro<'pr, N: Borrow<Node<'pr>>>(call: &CallNode, ancestors: &[N]) -> bool {
    call.receiver().is_none() && in_macro_scope(ancestors, span(&call.as_node()))
}

/// `in_macro_scope?` of a node whose parent is `ancestors.last()`, without naming the node: only
/// a statement (child of the program or of a `StatementsNode`) can be in macro scope. For a
/// BlockNode pass the ancestors without its call (parser's block wraps the call).
pub fn in_macro_scope_of_child<'pr, N: Borrow<Node<'pr>>>(ancestors: &[N]) -> bool {
    let Some((p, rest)) = ancestors.split_last() else { return true };
    let p = p.borrow();
    match kind::of(p) {
        kind::PROGRAM_NODE => true,
        kind::STATEMENTS_NODE => in_macro_scope(rest, span(p)),
        _ => false,
    }
}
