//! Shared parts of Style/SelectByKind, SelectByRange and SelectByRegexp.

use crate::helpers::block_args::Single;
use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

/// `receiver_allowed?`: a hash literal, `creates_hash?` or `ENV` (the block call's receiver).
pub fn receiver_allowed(recv: Option<Node>) -> bool {
    let Some(recv) = recv else { return false };
    recv.as_hash_node().is_some() || creates_hash(&recv) || is_top_const(&recv, "ENV")
}

/// ```text
/// {(call (const _ :Hash) {:new :[]} ...)
///  (block (call (const _ :Hash) :new ...) ...)
///  (call _ {:to_h :to_hash} ...)}
/// ```
fn creates_hash(node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    let name = method_name(&call);
    let hash_recv = call.receiver().is_some_and(|r| {
        r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Hash")
            || r.as_constant_path_node().is_some_and(|p| p.name().is_some_and(|n| n.as_slice() == b"Hash"))
    });
    match call.block().and_then(|b| b.as_block_node()) {
        // A plain `block` only: numblock/itblock do not match.
        Some(b) => {
            hash_recv
                && name == "new"
                && b.parameters().is_none_or(|p| p.as_block_parameters_node().is_some())
        }
        None => (hash_recv && matches!(name, "new" | "[]")) || matches!(name, "to_h" | "to_hash"),
    }
}

/// `(lvar %1)` for the block variable of `single_var`: `it` is Prism's ItLocalVariableReadNode.
pub fn is_block_var(node: &Node, single: Single, var: &[u8]) -> bool {
    match single {
        Single::It => node.as_it_local_variable_read_node().is_some(),
        _ => node.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == var),
    }
}

/// A parser `send` (not csend, no literal block).
pub fn plain_send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && !c.block().is_some_and(|b| b.as_block_node().is_some()))
}

/// The receiver of `(send x :!)`.
pub fn negation_receiver<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = plain_send(node)?;
    (method_name(&call) == "!" && send_args(&call).is_empty()).then(|| call.receiver()).flatten()
}

/// `(begin x)`: the single expression inside one level of parentheses.
pub fn paren_single<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let body = node.as_parentheses_node()?.body()?;
    let Some(s) = body.as_statements_node() else { return Some(body) };
    let mut it = s.body().iter();
    let first = it.next();
    if it.next().is_some() { None } else { first }
}
