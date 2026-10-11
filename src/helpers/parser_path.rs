//! Parser-gem ancestors of a node below the one a cop is visiting (`node.parent`,
//! `each_ancestor` on a descendant found by walking down from the visited node).

use crate::prelude::*;
use ruby_prism::{
    ArgumentsNode, BlockArgumentNode, BlockNode, CallNode, ElseNode, EnsureNode, ParametersNode, RescueNode, SplatNode,
    StatementsNode, Visit,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tag {
    Send,
    Csend,
    /// block / numblock / itblock / lambda literal (`any_block`)
    Block,
    Begin,
    Other,
}

/// A parser node as seen from a descendant.
#[derive(Clone, Copy, Debug)]
pub struct Frame<'pr> {
    pub tag: Tag,
    pub range: (usize, usize),
    /// send / csend: method name, call operator (`.`, `&.`, `::`), setter (`a.b = c`, `a[i] = c`)
    pub name: &'pr str,
    pub dot: Option<(usize, usize, &'pr [u8])>,
    pub setter: bool,
}

impl Frame<'_> {
    pub fn same(&self, other: &Frame) -> bool {
        self.tag == other.tag && self.range == other.range
    }
    pub fn is_call(&self) -> bool {
        matches!(self.tag, Tag::Send | Tag::Csend)
    }
}

fn plain<'pr>(tag: Tag, n: &Node) -> Frame<'pr> {
    let l = n.location();
    Frame { tag, range: (l.start_offset(), l.end_offset()), name: "", dot: None, setter: false }
}

fn send_frame<'pr>(call: &CallNode<'pr>) -> Frame<'pr> {
    let dot = call.call_operator_loc().map(|l| (l.start_offset(), l.end_offset(), l.as_slice()));
    Frame {
        tag: if is_csend(call) { Tag::Csend } else { Tag::Send },
        range: send_range(call),
        name: method_name(call),
        dot,
        setter: call.equal_loc().is_some(),
    }
}

fn has_block(n: &Node) -> bool {
    crate::helpers::node_types::literal_block(n).is_some()
}

/// The parser node a Prism node stands for when it is the subject (a call with a block is the block).
pub fn frame_of<'pr>(n: &Node<'pr>) -> Frame<'pr> {
    match n {
        _ if has_block(n) => plain(Tag::Block, n),
        Node::CallNode { .. } => send_frame(&n.as_call_node().unwrap()),
        Node::LambdaNode { .. } => plain(Tag::Block, n),
        Node::ParenthesesNode { .. } | Node::EmbeddedStatementsNode { .. } => plain(Tag::Begin, n),
        Node::StatementsNode { .. } if n.as_statements_node().unwrap().body().iter().count() > 1 => plain(Tag::Begin, n),
        _ => plain(Tag::Other, n),
    }
}

fn same_node(a: &Node, b: &Node) -> bool {
    let (la, lb) = (a.location(), b.location());
    kind::of(a) == kind::of(b) && la.start_offset() == lb.start_offset() && la.end_offset() == lb.end_offset()
}

/// Parser ancestors (outermost first) of `target`, whose Prism ancestors are `chain` (outermost first).
pub fn frames<'pr>(chain: &[&Node<'pr>], target: &Node<'pr>) -> Vec<Frame<'pr>> {
    let mut out = Vec::new();
    for (i, n) in chain.iter().enumerate() {
        let next = chain.get(i + 1).copied().unwrap_or(target);
        let prev = i.checked_sub(1).map(|p| chain[p]);
        match kind::of(n) {
            kind::PROGRAM_NODE | kind::ARGUMENTS_NODE | kind::ELSE_NODE => {}
            kind::STATEMENTS_NODE => {
                let wrapped = prev.is_some_and(|p| {
                    matches!(kind::of(p), kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE)
                        || p.as_begin_node().is_some_and(|b| b.begin_keyword_loc().is_some())
                });
                if !wrapped && n.as_statements_node().unwrap().body().iter().count() > 1 {
                    out.push(plain(Tag::Begin, n));
                }
            }
            kind::BLOCK_NODE => {
                if let Some(p) = prev {
                    out.push(plain(Tag::Block, p));
                }
            }
            _ if has_block(n) => {
                if next.as_block_node().is_none() {
                    out.push(plain(Tag::Block, n));
                    out.push(match n.as_call_node() {
                        Some(c) => send_frame(&c),
                        None => plain(Tag::Other, n),
                    });
                }
            }
            _ => out.push(frame_of(n)),
        }
    }
    out
}

struct Finder<'a, 'pr> {
    outer: &'a [Node<'pr>],
    target: (usize, usize, usize),
    stack: Vec<Node<'pr>>,
    found: Option<Vec<Frame<'pr>>>,
}

impl<'pr> Finder<'_, 'pr> {
    fn check(&mut self, node: &Node<'pr>) {
        let l = node.location();
        if self.found.is_none() && (kind::of(node), l.start_offset(), l.end_offset()) == self.target {
            let chain: Vec<&Node<'pr>> = self.outer.iter().chain(self.stack.iter()).collect();
            self.found = Some(frames(&chain, node));
        }
    }

    fn branch(&mut self, node: Node<'pr>, walk: impl FnOnce(&mut Self)) {
        if self.stack.last().is_some_and(|top| same_node(top, &node)) {
            walk(self);
        } else {
            self.visit_branch_node_enter(node);
            walk(self);
            self.visit_branch_node_leave();
        }
    }
}

impl<'pr> Visit<'pr> for Finder<'_, 'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
        self.stack.push(node);
    }
    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
    fn visit_arguments_node(&mut self, node: &ArgumentsNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_arguments_node(s, node));
    }
    fn visit_block_argument_node(&mut self, node: &BlockArgumentNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_block_argument_node(s, node));
    }
    fn visit_block_node(&mut self, node: &BlockNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_block_node(s, node));
    }
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_call_node(s, node));
    }
    fn visit_else_node(&mut self, node: &ElseNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_else_node(s, node));
    }
    fn visit_ensure_node(&mut self, node: &EnsureNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_ensure_node(s, node));
    }
    fn visit_parameters_node(&mut self, node: &ParametersNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_parameters_node(s, node));
    }
    fn visit_rescue_node(&mut self, node: &RescueNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_rescue_node(s, node));
    }
    fn visit_splat_node(&mut self, node: &SplatNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_splat_node(s, node));
    }
    fn visit_statements_node(&mut self, node: &StatementsNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_statements_node(s, node));
    }
}

/// Parser ancestors of `target` (a descendant of `root`, or `root` itself), nearest first.
/// `outer` are root's Prism ancestors (`cx.ancestors`).
pub fn ancestors<'pr>(outer: &[Node<'pr>], root: &Node<'pr>, target: &Node<'pr>) -> Option<Vec<Frame<'pr>>> {
    let l = target.location();
    let mut f = Finder { outer, target: (kind::of(target), l.start_offset(), l.end_offset()), stack: Vec::new(), found: None };
    f.visit(root);
    let mut v = f.found?;
    v.reverse();
    Some(v)
}
