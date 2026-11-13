//! rubocop-thread_safety shared code: parser-gem-shaped ancestors and the
//! OperationWithThreadsafeResult mixin.

use crate::helpers::node_types::plain_block_params;
use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;
use ruby_prism::CallNode;

/// A parser-gem ancestor: a `block` (or `numblock`/`itblock`, `plain` false) with the node that owns
/// it (its call, `super`, or the lambda itself), or any other node.
pub enum Anc<'a, 'pr> {
    Block { owner: &'a Node<'pr>, body: Option<Node<'pr>>, plain: bool },
    Node(&'a Node<'pr>),
}

impl<'pr> Anc<'_, 'pr> {
    /// The send the parser `block` wraps.
    pub fn block_call(&self) -> Option<CallNode<'pr>> {
        match self {
            Anc::Block { owner, .. } => owner.as_call_node(),
            Anc::Node(_) => None,
        }
    }

    /// `block_type?`
    pub fn is_block(&self) -> bool {
        matches!(self, Anc::Block { plain: true, .. })
    }

    pub fn node(&self) -> Option<&Node<'pr>> {
        match self {
            Anc::Node(n) => Some(n),
            Anc::Block { .. } => None,
        }
    }

    /// `children.first` when it is a parser `send`/`csend` (`AST::SendNode`).
    // ponytail: the first child of the node types a send can head; others read as not a send.
    pub fn first_send(&self) -> Option<CallNode<'pr>> {
        let n = match self {
            Anc::Block { owner, .. } => return owner.as_call_node(),
            Anc::Node(n) => *n,
        };
        let first = match n {
            Node::CallNode { .. } => n.as_call_node().unwrap().receiver(),
            Node::IfNode { .. } => Some(n.as_if_node().unwrap().predicate()),
            Node::UnlessNode { .. } => Some(n.as_unless_node().unwrap().predicate()),
            Node::WhileNode { .. } => Some(n.as_while_node().unwrap().predicate()),
            Node::UntilNode { .. } => Some(n.as_until_node().unwrap().predicate()),
            Node::AndNode { .. } => Some(n.as_and_node().unwrap().left()),
            Node::OrNode { .. } => Some(n.as_or_node().unwrap().left()),
            Node::StatementsNode { .. } => n.as_statements_node().unwrap().body().iter().next(),
            Node::ArrayNode { .. } => n.as_array_node().unwrap().elements().iter().next(),
            Node::ReturnNode { .. } => n.as_return_node().unwrap().arguments().and_then(|a| a.arguments().iter().next()),
            Node::ParenthesesNode { .. } => n.as_parentheses_node().unwrap().body().and_then(|b| match b.as_statements_node() {
                Some(s) => s.body().iter().next(),
                None => Some(b),
            }),
            _ => None,
        }?;
        first.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
    }
}

/// The parser-gem ancestors of the node whose Prism ancestors are `ancestors`, nearest first.
pub fn parser_ancestors<'a, 'pr>(ancestors: &'a [Node<'pr>]) -> Vec<Anc<'a, 'pr>> {
    let mut out = Vec::new();
    let mut i = ancestors.len();
    while i > 0 {
        i -= 1;
        let n = &ancestors[i];
        match n {
            Node::BlockNode { .. } if i > 0 => {
                let b = n.as_block_node().unwrap();
                out.push(Anc::Block { owner: &ancestors[i - 1], body: b.body(), plain: plain_block_params(b.parameters()) });
                i -= 1;
            }
            Node::LambdaNode { .. } => {
                let l = n.as_lambda_node().unwrap();
                out.push(Anc::Block { owner: n, body: l.body(), plain: plain_block_params(l.parameters()) });
            }
            Node::CallNode { .. } => {
                out.push(Anc::Node(n));
                // Inside the receiver or arguments of a call with a block: parser's block wraps it.
                if let Some(b) = n.as_call_node().unwrap().block().and_then(|b| b.as_block_node()) {
                    out.push(Anc::Block { owner: n, body: b.body(), plain: plain_block_params(b.parameters()) });
                }
            }
            _ if frame(n) == Frame::Transparent => {}
            _ => out.push(Anc::Node(n)),
        }
    }
    out
}

/// A receiver-less `def` (parser `def`, not `defs`).
pub fn is_def(node: &Node) -> bool {
    node.as_def_node().is_some_and(|d| d.receiver().is_none())
}

/// `(const _ _)` names, as rubocop-ast `const_name`.
pub fn const_name(node: &Node) -> Option<String> {
    if let Some(c) = node.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let p = node.as_constant_path_node()?;
    let short = String::from_utf8_lossy(p.name()?.as_slice()).into_owned();
    Some(match p.parent() {
        None => short,
        Some(ns) => format!("{}::{short}", const_name(&ns).unwrap_or_default()),
    })
}

/// A `send` (not csend) without a literal block.
fn send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// The call of a parser `block` node (a call with a `{ }`/`do` block taking ordinary parameters).
fn block_send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| {
        !is_csend(c) && c.block().and_then(|b| b.as_block_node()).is_some_and(|b| plain_block_params(b.parameters()))
    })
}

/// The constant path `(const ... (const {nil? cbase} :Root) ...)` of `depth` segments under a
/// top-level `root` (any names after it); `depth` 1 is the root itself.
fn const_under(node: &Node, root: &str, depth: usize) -> bool {
    if depth == 1 {
        return is_top_const(node, root);
    }
    node.as_constant_path_node().is_some_and(|p| p.parent().is_some_and(|q| const_under(&q, root, depth - 1)))
}

/// OperationWithThreadsafeResult#operation_produces_threadsafe_object?
pub fn operation_produces_threadsafe_object(node: &Node) -> bool {
    let new_on = |c: &CallNode, f: &dyn Fn(&Node) -> bool| method_name(c) == "new" && c.receiver().is_some_and(|r| f(&r));
    let threadsafe = |r: &Node| {
        (const_under(r, "ThreadSafe", 2)
            && r.as_constant_path_node().unwrap().name().is_some_and(|n| matches!(n.as_slice(), b"Hash" | b"Array")))
            || (2..=4).any(|d| const_under(r, "Concurrent", d))
    };
    if let Some(c) = send(node) {
        return new_on(&c, &|r| is_top_const(r, "Queue") || threadsafe(r));
    }
    block_send(node).is_some_and(|c| new_on(&c, &threadsafe))
}

/// `node.each_node`: the node and its descendants, in pre-order.
pub fn each_node<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    struct All<'pr>(Vec<Node<'pr>>);
    impl<'pr> ruby_prism::Visit<'pr> for All<'pr> {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            self.0.push(node);
        }
        fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
            self.0.push(node);
        }
    }
    let mut all = All(Vec::new());
    ruby_prism::Visit::visit(&mut all, node);
    all.0
}
