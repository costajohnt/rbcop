//! What RuboCop's CommentConfig knows about a file's lines, for cops that inspect directives
//! (Style/DirectiveScope). The runner uses the same `code_lines` and
//! `StatementBounds` for its directive ranges.

use crate::cop::Facts;
use crate::prelude::*;
use crate::source::Source;
use ruby_prism::Visit;
use std::collections::HashMap;

/// CommentConfig#non_comment_token_line_numbers as `code[line]`. Parser emits a token per line of
/// string/heredoc content, so those lines count too.
pub fn code_lines(src: &Source, facts: &Facts) -> Vec<bool> {
    let mut code = facts.string_lines.clone();
    code.resize(src.line_count() + 2, false);
    for t in src.tokens() {
        let skip = matches!(
            t.kind,
            sys::PM_TOKEN_COMMENT
                | sys::PM_TOKEN_EMBDOC_BEGIN
                | sys::PM_TOKEN_EMBDOC_LINE
                | sys::PM_TOKEN_EMBDOC_END
                | sys::PM_TOKEN_NEWLINE
                | sys::PM_TOKEN_IGNORED_NEWLINE
                | sys::PM_TOKEN_EOF
                | sys::PM_TOKEN___END__
        );
        if !skip && t.end > t.start {
            code[src.line_of(t.start)] = true;
        }
    }
    code
}

/// DisableNext#statement_bounds_at for every line: the statement starting there with the greatest
/// last line (first in pre-order on ties) and the last line of its subtree, where heredocs count
/// up to their terminator.
#[derive(Default)]
pub struct StatementBounds {
    best: HashMap<usize, (usize, usize, usize)>, // first line -> (last line, preorder, subtree end)
}

impl StatementBounds {
    pub fn build(src: &Source, program: &Node) -> Self {
        let mut w =
            BoundsWalker { src, open: Vec::new(), best: HashMap::new(), counter: 0, top: Vec::new(), leaf: None };
        w.visit(program);
        StatementBounds { best: w.best }
    }

    pub fn at(&self, line: usize) -> (usize, usize) {
        self.best.get(&line).map_or((line, line), |&(_, _, end)| (line, end.max(line)))
    }
}

struct BoundsWalker<'s, 'a> {
    src: &'s Source<'a>,
    open: Vec<(usize, usize, usize, usize, bool)>, // (preorder, first line, last line, subtree end, statement-like)
    best: HashMap<usize, (usize, usize, usize)>,
    counter: usize,
    /// Start/end/kind of the open branch nodes, to avoid entering a node twice.
    top: Vec<(usize, usize, usize)>,
    /// The leaf being entered through the generic hook (its typed visit must not count it again).
    leaf: Option<(usize, usize, usize)>,
}

fn key(node: &Node) -> (usize, usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset(), kind::of(node))
}

impl BoundsWalker<'_, '_> {
    fn enter(&mut self, node: &Node) {
        let loc = node.location();
        let first = self.src.line_of(loc.start_offset());
        let last = self.src.line_of(loc.end_offset().max(loc.start_offset()));
        let end = heredoc_end_line(self.src, node).unwrap_or(last);
        // Parser `begin` nodes (multi-statement bodies, parens) and Prism-only wrappers are skipped.
        let statement_like = !matches!(
            kind::of(node),
            kind::STATEMENTS_NODE
                | kind::PARENTHESES_NODE
                | kind::PROGRAM_NODE
                | kind::ARGUMENTS_NODE
                | kind::ELSE_NODE
                | kind::EMBEDDED_STATEMENTS_NODE
        );
        self.counter += 1;
        self.open.push((self.counter, first, last, end, statement_like));
    }

    /// A parser node Prism has no node for (`rescue`/`ensure` around a body), as a statement.
    fn enter_span(&mut self, start: usize, end: usize) {
        let (first, last) = (self.src.line_of(start), self.src.line_of(end.max(start)));
        self.counter += 1;
        self.open.push((self.counter, first, last, last, true));
    }

    fn leave(&mut self) {
        let Some((order, first, last, end, statement_like)) = self.open.pop() else { return };
        if let Some(parent) = self.open.last_mut() {
            parent.3 = parent.3.max(end);
        }
        if statement_like {
            let better = match self.best.get(&first) {
                None => true,
                Some(&(l, o, _)) => last > l || (last == l && order < o),
            };
            if better {
                self.best.insert(first, (last, order, end));
            }
        }
    }

    /// For node types Prism's default visitor reaches without the enter/leave hooks.
    fn branch<'pr>(&mut self, node: Node<'pr>, walk: impl FnOnce(&mut Self)) {
        if self.top.last() == Some(&key(&node)) {
            walk(self);
        } else {
            self.visit_branch_node_enter(node);
            walk(self);
            self.visit_branch_node_leave();
        }
    }

    fn leaf(&mut self, node: &Node) {
        if self.leaf != Some(key(node)) {
            self.enter(node);
            self.leave();
        }
    }
}

/// Terminator line of a heredoc node (parser's `loc.heredoc_end`).
fn heredoc_end_line(src: &Source, node: &Node) -> Option<usize> {
    if !is_heredoc(node) {
        return None;
    }
    let closing = match kind::of(node) {
        kind::STRING_NODE => node.as_string_node()?.closing_loc(),
        kind::INTERPOLATED_STRING_NODE => node.as_interpolated_string_node()?.closing_loc(),
        kind::X_STRING_NODE => Some(node.as_x_string_node()?.closing_loc()),
        kind::INTERPOLATED_X_STRING_NODE => Some(node.as_interpolated_x_string_node()?.closing_loc()),
        _ => None,
    }?;
    Some(src.line_of(closing.start_offset()))
}

macro_rules! branch_visits {
    ($($method:ident: $ty:ident => $walk:ident),* $(,)?) => {
        $(fn $method(&mut self, node: &ruby_prism::$ty<'pr>) {
            self.branch(node.as_node(), |s| ruby_prism::$walk(s, node));
        })*
    };
}

impl<'pr> Visit<'pr> for BoundsWalker<'_, '_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.enter(&node);
        self.top.push(key(&node));
    }
    fn visit_branch_node_leave(&mut self) {
        self.leave();
        self.top.pop();
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.enter(&node);
        self.leave();
        self.leaf = Some(key(&node));
    }
    fn visit_leaf_node_leave(&mut self) {
        self.leaf = None;
    }
    /// Parser wraps a body with rescue clauses in a `rescue` node (body start to the last clause or
    /// `else` body) and that in an `ensure` node (to the ensure body); both start on the body's line.
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        let (rescue, ensure) = (node.rescue_clause(), node.ensure_clause());
        if rescue.is_none() && ensure.is_none() {
            return ruby_prism::visit_begin_node(self, node);
        }
        let span = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
        let stmts = node.statements();
        let mut inner = stmts.as_ref().map(|s| span(s.location()));
        if let Some(r) = &rescue {
            let end = match node.else_clause() {
                Some(e) => e.statements().map_or(span(e.else_keyword_loc()).1, |s| span(s.location()).1),
                None => span(r.location()).1,
            };
            inner = Some((inner.map_or(span(r.location()).0, |i| i.0), end));
        }
        if let Some(e) = &ensure {
            let kw = span(e.ensure_keyword_loc());
            let end = e.statements().map_or(kw.1, |s| span(s.location()).1);
            self.enter_span(inner.map_or(kw.0, |i| i.0), end);
        }
        if let Some((s, e)) = inner.filter(|_| rescue.is_some()) {
            self.enter_span(s, e);
        }
        if let Some(s) = &stmts {
            self.visit_statements_node(s);
        }
        if let Some(r) = &rescue {
            self.visit_rescue_node(r);
        }
        if let Some(e) = node.else_clause() {
            self.visit_else_node(&e);
        }
        if rescue.is_some() {
            self.leave();
        }
        if let Some(e) = &ensure {
            self.visit_ensure_node(e);
            self.leave();
        }
    }
    fn visit_block_parameter_node(&mut self, node: &ruby_prism::BlockParameterNode<'pr>) {
        self.leaf(&node.as_node());
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.leaf(&node.as_node());
    }
    branch_visits! {
        visit_arguments_node: ArgumentsNode => visit_arguments_node,
        visit_block_argument_node: BlockArgumentNode => visit_block_argument_node,
        visit_block_node: BlockNode => visit_block_node,
        visit_call_node: CallNode => visit_call_node,
        visit_constant_path_node: ConstantPathNode => visit_constant_path_node,
        visit_else_node: ElseNode => visit_else_node,
        visit_ensure_node: EnsureNode => visit_ensure_node,
        visit_parameters_node: ParametersNode => visit_parameters_node,
        visit_rescue_node: RescueNode => visit_rescue_node,
        visit_splat_node: SplatNode => visit_splat_node,
        visit_statements_node: StatementsNode => visit_statements_node,
    }
}
