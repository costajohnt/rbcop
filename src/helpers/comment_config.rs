//! What RuboCop's CommentConfig knows about a file's lines, for cops that inspect directives
//! (Style/DirectiveScope). The runner uses the same `code_lines` and
//! `StatementBounds` for its directive ranges.

use crate::cop::Facts;
use crate::helpers::resbody;
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

/// First and last line of a node.
type Lines = (usize, usize);

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
        // Parser's `resbody` ends at its own body; Prism's RescueNode spans the following clauses.
        let end_offset = match node.as_rescue_node() {
            Some(r) => resbody::range(self.src, &r).1,
            None => loc.end_offset(),
        };
        let last = self.src.line_of(end_offset.max(loc.start_offset()));
        let end = heredoc_end_line(self.src, node).unwrap_or(last);
        // Parser `begin` nodes (multi-statement bodies, parens) and Prism-only wrappers are skipped.
        // Parser's `ensure` starts at the body, not the keyword (see `visit_begin_node`).
        let statement_like = !matches!(
            kind::of(node),
            kind::STATEMENTS_NODE
                | kind::PARENTHESES_NODE
                | kind::PROGRAM_NODE
                | kind::ARGUMENTS_NODE
                | kind::ELSE_NODE
                | kind::ENSURE_NODE
                | kind::EMBEDDED_STATEMENTS_NODE
        );
        self.push(first, last, end, statement_like);
    }

    fn push(&mut self, first: usize, last: usize, end: usize, statement_like: bool) {
        self.counter += 1;
        self.open.push((self.counter, first, last, end, statement_like));
    }

    /// Parser's `rescue` and `ensure` nodes (eh_keyword_map), which Prism folds into BeginNode:
    /// `rescue` runs from the body (else the first `rescue`) to the else branch (else the last
    /// resbody); `ensure` from the body or `rescue` (else the keyword) to the ensure body (else the
    /// keyword). Returns (rescue lines, ensure lines).
    fn eh_lines(&self, node: &ruby_prism::BeginNode) -> (Option<Lines>, Option<Lines>) {
        let body_start = node.statements().map(|s| s.location().start_offset());
        let rescue = node.rescue_clause().map(|r| {
            let start = body_start.unwrap_or(r.keyword_loc().start_offset());
            let mut last = r;
            while let Some(next) = last.subsequent() {
                last = next;
            }
            let end = match node.else_clause() {
                Some(e) => e.statements().map_or(e.else_keyword_loc().end_offset(), |s| s.location().end_offset()),
                None => resbody::range(self.src, &last).1,
            };
            (start, end)
        });
        let ensure = node.ensure_clause().map(|e| {
            let start = rescue.map(|r| r.0).or(body_start).unwrap_or(e.ensure_keyword_loc().start_offset());
            let end = e.statements().map_or(e.ensure_keyword_loc().end_offset(), |s| s.location().end_offset());
            (start, end)
        });
        let lines = |(s, e): (usize, usize)| (self.src.line_of(s), self.src.line_of(e.max(s)));
        (rescue.map(lines), ensure.map(lines))
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
    fn visit_block_parameter_node(&mut self, node: &ruby_prism::BlockParameterNode<'pr>) {
        self.leaf(&node.as_node());
    }
    /// Parser's resbodies are siblings: the next clause is not inside this one's subtree.
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode<'pr>) {
        self.branch(node.as_node(), |s| {
            for e in node.exceptions().iter() {
                s.visit(&e);
            }
            if let Some(r) = node.reference() {
                s.visit(&r);
            }
            if let Some(st) = node.statements() {
                s.visit_statements_node(&st);
            }
        });
        if let Some(next) = node.subsequent() {
            self.visit_rescue_node(&next);
        }
    }
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        self.branch(node.as_node(), |s| {
            let (rescue, ensure) = s.eh_lines(node);
            // Parser nesting and pre-order: (ensure (rescue body resbody... else) ensure_body).
            if let Some((first, last)) = ensure {
                s.push(first, last, last, true);
            }
            if let Some((first, last)) = rescue {
                s.push(first, last, last, true);
            }
            if let Some(st) = node.statements() {
                s.visit_statements_node(&st);
            }
            if let Some(r) = node.rescue_clause() {
                s.visit_rescue_node(&r);
            }
            if let Some(e) = node.else_clause() {
                s.visit_else_node(&e);
            }
            if rescue.is_some() {
                s.leave();
            }
            if let Some(e) = node.ensure_clause() {
                s.visit_ensure_node(&e);
            }
            if ensure.is_some() {
                s.leave();
            }
        });
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
        visit_splat_node: SplatNode => visit_splat_node,
        visit_statements_node: StatementsNode => visit_statements_node,
    }
}
