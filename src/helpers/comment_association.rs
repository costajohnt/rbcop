//! A slice of parser's `Comment::Associator` (`processed_source.ast_with_comments[node]`) for
//! statements: the leading comments a statement collects. Interior comments are always taken by
//! the statement or its descendants and decorating comments sit on its own lines, so for whole-line
//! ranges (`range_with_comments_and_lines`) the leading comments are all that matter.
//! `Associator` runs the whole walk over a subtree (`ast_with_comments[node]` for any node in it).

use crate::source::{Comment, Source};
use regex::Regex;
use std::sync::LazyLock;

static MAGIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#\s*(?:-\*-\s*(?:frozen_string_literal|warn_indent|warn_past_scope):.*-\*-$|\s*(?:frozen_string_literal|warn_indent|warn_past_scope):)")
        .unwrap()
});
static ENCODING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\s#](en)?coding\s*[:=]\s*[A-Za-z0-9_-]").unwrap());

/// Number of leading comments `advance_through_directives` skips (shebang, magic, encoding).
pub fn skipped_directives(src: &Source) -> usize {
    let text = |i: usize| src.comments.get(i).map(|c| src.slice(c.start, c.end));
    let mut i = 0;
    if text(i).is_some_and(|t| t.starts_with("#!")) {
        i += 1;
    }
    if text(i).is_some_and(|t| MAGIC.is_match(t)) {
        i += 1;
    }
    if text(i).is_some_and(|t| ENCODING.is_match(t)) {
        i += 1;
    }
    i
}

/// Leading comments of a statement starting at `start`: comments ending before it that come after
/// `after` (byte offset) and lie on a line below `after_line` (comments on the previous sibling's
/// last line decorate that sibling).
pub fn leading<'a>(src: &'a Source, start: usize, after: usize, after_line: Option<usize>) -> Vec<&'a Comment> {
    let skip = skipped_directives(src);
    // Comments are sorted and disjoint: binary-search the window instead of scanning them all.
    let lo = src.comments.partition_point(|c| c.start < after).max(skip);
    let hi = src.comments.partition_point(|c| c.end <= start);
    src.comments
        .get(lo..hi)
        .unwrap_or_default()
        .iter()
        .filter(|c| after_line.is_none_or(|l| src.line_of(c.start) > l))
        .collect()
}

/// Parser's `Comment::Associator` walk over a subtree: the comments each node gets (leading ones
/// before it, then after its children the interior ones before its end and those decorating its
/// last line), as (start, end, comment indices) per parser node in visit order. `cursor` is the
/// first comment still unassigned. Prism's ProgramNode/ArgumentsNode/StatementsNode wrappers are skipped and
/// a parenthesised `begin` takes no leading comments.
pub struct Associator<'a> {
    pub src: &'a Source<'a>,
    pub cursor: usize,
    pub stack: Vec<Option<usize>>,
    pub nodes: Vec<(usize, usize, Vec<usize>)>,
}

impl<'a> Associator<'a> {
    pub fn new(src: &'a Source<'a>, cursor: usize) -> Self {
        Associator { src, cursor, stack: Vec::new(), nodes: Vec::new() }
    }

    fn comment(&self) -> Option<Comment> {
        self.src.comments.get(self.cursor).copied()
    }

    pub fn leading(&mut self, i: usize) {
        while self.comment().is_some_and(|c| c.end <= self.nodes[i].0) {
            self.nodes[i].2.push(self.cursor);
            self.cursor += 1;
        }
    }

    pub fn trailing(&mut self, i: usize) {
        while self.comment().is_some_and(|c| c.end <= self.nodes[i].1) {
            self.nodes[i].2.push(self.cursor);
            self.cursor += 1;
        }
        let last_line = self.src.line_of(self.nodes[i].1);
        while self.comment().is_some_and(|c| self.src.line_of(c.start) == last_line) {
            self.nodes[i].2.push(self.cursor);
            self.cursor += 1;
        }
    }

    /// Enter a parser node spanning `start..end` (also one Prism has no node for, such as the
    /// `lvasgn` left side of `x ||= y`).
    pub fn enter_range(&mut self, start: usize, end: usize, leading: bool) {
        self.nodes.push((start, end, Vec::new()));
        let i = self.nodes.len() - 1;
        if leading {
            self.leading(i);
        }
        self.stack.push(Some(i));
    }

    pub fn enter(&mut self, node: &ruby_prism::Node) {
        let k = crate::node_kind::of(node);
        // Parser has no program node: its root is the lone statement or a `begin` (which takes no
        // leading comments), so the file-leading comments go to the first statement (#356).
        if matches!(k, crate::node_kind::ARGUMENTS_NODE | crate::node_kind::STATEMENTS_NODE | crate::node_kind::PROGRAM_NODE) {
            self.stack.push(None);
            return;
        }
        // Parser's `rescue`/`ensure` node for a body with clauses starts at the first statement,
        // not at the `def`/`do` Prism's implicit BeginNode starts at (#326).
        if k == crate::node_kind::BEGIN_NODE && node.as_begin_node().unwrap().begin_keyword_loc().is_none() {
            let (s, e) = crate::ast::body_range(node);
            return self.enter_range(s, e, true);
        }
        let loc = node.location();
        self.enter_range(loc.start_offset(), loc.end_offset(), k != crate::node_kind::PARENTHESES_NODE);
    }

    pub fn leave(&mut self) {
        if let Some(Some(i)) = self.stack.pop() {
            self.trailing(i);
        }
    }

    /// The comments of the first node spanning `start..end`.
    pub fn comments_of(&self, start: usize, end: usize) -> impl Iterator<Item = &'a Comment> + '_ {
        let src = self.src;
        self.nodes.iter().find(|n| n.0 == start && n.1 == end).into_iter().flat_map(move |n| n.2.iter().map(move |&c| &src.comments[c]))
    }
}

impl<'pr> ruby_prism::Visit<'pr> for Associator<'_> {
    fn visit_branch_node_enter(&mut self, node: ruby_prism::Node<'pr>) {
        self.enter(&node);
    }
    fn visit_branch_node_leave(&mut self) {
        self.leave();
    }
    fn visit_leaf_node_enter(&mut self, node: ruby_prism::Node<'pr>) {
        self.enter(&node);
    }
    fn visit_leaf_node_leave(&mut self) {
        self.leave();
    }
    /// `def m()` without parameters: parser's `(args)` spans the parentheses and takes the comment
    /// decorating that line; Prism has no node for them (#326).
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        if let Some(r) = node.receiver() {
            self.visit(&r);
        }
        match node.parameters() {
            Some(p) => self.visit_parameters_node(&p),
            None => {
                if let (Some(l), Some(r)) = (node.lparen_loc(), node.rparen_loc()) {
                    self.enter_range(l.start_offset(), r.end_offset(), true);
                    self.leave();
                }
            }
        }
        if let Some(b) = node.body() {
            self.visit(&b);
        }
    }
    /// A call with a literal block is parser's `block` whose first child is the `send`: a comment
    /// decorating the send's last line (`foo do # c`) belongs to the send (#326).
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let block = node.block().filter(|b| b.as_block_node().is_some());
        if block.is_some() {
            let (s, e) = crate::ast::send_range(node);
            self.enter_range(s, e, true);
        }
        if let Some(r) = node.receiver() {
            self.visit(&r);
        }
        if let Some(a) = node.arguments() {
            self.visit_arguments_node(&a);
        }
        match block {
            Some(b) => {
                self.leave();
                self.visit(&b);
            }
            None => {
                if let Some(b) = node.block() {
                    self.visit(&b);
                }
            }
        }
    }
}
