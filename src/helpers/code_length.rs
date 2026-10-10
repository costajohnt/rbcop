//! RuboCop's CodeLength mixin and Metrics::Utils::CodeLengthCalculator (Metrics/BlockLength,
//! ClassLength, MethodLength, ModuleLength).

use crate::ast::send_range;
use crate::cop::Cx;
use crate::node_kind as kind;
use crate::source::Source;
use ruby_prism::{CallNode, Node, Visit};

/// The parser node CodeLength measures.
pub enum Measured<'a, 'pr> {
    /// `class` / `module`: lines between the keyword and `end`, minus nested classes and modules.
    Classlike(&'a Node<'pr>),
    /// def / block / sclass / casgn: the lines of `body`. `root` is the node whose descendants
    /// `CountAsOne` folds (a block is its CallNode).
    Body { root: &'a Node<'pr>, body: Option<Node<'pr>> },
}

/// `check_code_length`: `span` is the measured node's range (for `line_count`), `offense` the
/// reported range, `msg(length, max)` the message.
pub fn check(cx: &mut Cx, m: Measured, span: (usize, usize), offense: (usize, usize), msg: impl Fn(usize, i64) -> String) {
    let max = cx.cfg.int("Max").unwrap_or(0);
    let line_count = cx.src.line_of(span.1) - cx.src.line_of(span.0) + 1;
    if line_count as i64 <= max {
        return;
    }
    let mut folds = Folds::default();
    for t in cx.cfg.strings("CountAsOne") {
        match t.as_str() {
            "array" => folds.array = true,
            "hash" => folds.hash = true,
            "heredoc" => folds.heredoc = true,
            "method_call" => folds.call = true,
            // RuboCop raises on unknown foldable types (no offense).
            _ => return,
        }
    }
    let calc = Calc { src: cx.src, count_comments: cx.cfg.bool("CountComments").unwrap_or(false) };
    let (mut length, root) = match m {
        Measured::Classlike(node) => (calc.classlike_length(node) as i64, node),
        Measured::Body { root, body } => (body.map_or(0, |b| calc.view_length(&View::of_body(&b))) as i64, root),
    };
    if folds.any() {
        let mut f = Folder { calc: &calc, folds, root: ident(root), delta: 0 };
        f.visit(root);
        length += f.delta;
    }
    if length <= max {
        return;
    }
    cx.add(offense.0, offense.1, msg(length as usize, max));
}

#[derive(Default, Clone, Copy)]
struct Folds {
    array: bool,
    hash: bool,
    heredoc: bool,
    call: bool,
}

impl Folds {
    fn any(&self) -> bool {
        self.array || self.hash || self.heredoc || self.call
    }
}

/// String#blank?: empty or only whitespace (Ruby `lstrip` also strips NUL).
fn blank(line: &str) -> bool {
    line.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | 0))
}

/// `comment_line?`: `/^\s*#/`.
pub fn comment_line(line: &str) -> bool {
    line.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']).starts_with('#')
}

fn ident(node: &Node) -> (usize, usize, usize) {
    let l = node.location();
    (kind::of(node), l.start_offset(), l.end_offset())
}

pub fn is_heredoc_node(node: &Node) -> bool {
    crate::ast::is_heredoc(node)
}

/// Line of a heredoc's terminator (`loc.heredoc_end.line`).
fn heredoc_end_line(src: &Source, node: &Node) -> Option<usize> {
    let closing = match kind::of(node) {
        kind::STRING_NODE => node.as_string_node()?.closing_loc(),
        kind::INTERPOLATED_STRING_NODE => node.as_interpolated_string_node()?.closing_loc(),
        kind::X_STRING_NODE => Some(node.as_x_string_node()?.closing_loc()),
        kind::INTERPOLATED_X_STRING_NODE => Some(node.as_interpolated_x_string_node()?.closing_loc()),
        _ => None,
    }?;
    Some(src.line_of(closing.start_offset()))
}

/// A parser-gem node whose source and descendants are measured: its range and the Prism nodes
/// holding its descendants.
enum View<'a, 'pr> {
    /// A Prism node (itself excluded from its descendants).
    Node { range: (usize, usize), node: &'a Node<'pr> },
    /// A parser node Prism has no node for (the `send` of a block call or of `a.b ||= c`, a
    /// `rescue` exception list): its range and the Prism nodes inside it.
    Parts { range: (usize, usize), parts: Vec<Node<'pr>> },
}

impl<'a, 'pr> View<'a, 'pr> {
    fn of_body(body: &'a Node<'pr>) -> Self {
        View::Node { range: crate::ast::body_range(body), node: body }
    }
}

struct Calc<'s> {
    src: &'s Source<'s>,
    count_comments: bool,
}

impl Calc<'_> {
    fn irrelevant(&self, line: &str) -> bool {
        blank(line) || (!self.count_comments && comment_line(line))
    }

    fn count(&self, text: &str) -> usize {
        text.split_inclusive('\n').filter(|l| !self.irrelevant(l)).count()
    }

    /// `code_length` of a non-classlike node's body.
    fn view_length(&self, view: &View) -> usize {
        // A single-statement body is that statement in parser.
        if let View::Node { node, .. } = view
            && let Some(s) = node.as_statements_node()
            && s.body().iter().count() == 1
        {
            let stmt = s.body().iter().next().unwrap();
            return self.view_length(&View::Node { range: crate::ast::body_range(&stmt), node: &stmt });
        }
        let mut d = Descendants { src: self.src, root: None, max_line: 0, heredoc: false };
        let range = match view {
            View::Node { range, node } => {
                d.root = Some(ident(node));
                d.visit(node);
                *range
            }
            View::Parts { range, parts } => {
                for p in parts {
                    d.visit(p);
                }
                *range
            }
        };
        if d.heredoc {
            // `processed_source[(first_line - 1)..(last_line - 1)]`
            let first = self.src.line_of(range.0);
            if d.max_line < first {
                return 0;
            }
            let lines = &self.src.lines;
            return (first - 1..d.max_line.min(lines.len()))
                .filter(|&i| !self.irrelevant(lines[i]))
                .count();
        }
        self.count(&self.src.text[range.0..range.1])
    }

    /// `heredoc_length`: body lines plus the opening and the terminator.
    fn heredoc_length(&self, node: &Node) -> usize {
        let Some(term) = heredoc_end_line(self.src, node) else { return 0 };
        let start = match kind::of(node) {
            kind::STRING_NODE => Some(node.as_string_node().unwrap().content_loc().start_offset()),
            kind::INTERPOLATED_STRING_NODE => {
                node.as_interpolated_string_node().unwrap().parts().iter().next().map(|p| p.location().start_offset())
            }
            _ => None,
        };
        let body = start.map_or(0, |s| {
            (self.src.line_of(s)..term).filter(|&l| !self.irrelevant(self.src.lines[l - 1])).count()
        });
        body + 2
    }

    fn classlike_length(&self, node: &Node) -> usize {
        let body = node
            .as_class_node()
            .and_then(|c| c.body())
            .or_else(|| node.as_module_node().and_then(|m| m.body()));
        // namespace_module?: the body is a single class or module.
        if let Some(s) = body.as_ref().and_then(|b| b.as_statements_node()) {
            let stmts: Vec<_> = s.body().iter().collect();
            if stmts.len() == 1 && matches!(kind::of(&stmts[0]), kind::CLASS_NODE | kind::MODULE_NODE) {
                return 0;
            }
        }
        let loc = node.location();
        let (first, last) = (self.src.line_of(loc.start_offset()), self.src.line_of(loc.end_offset()));
        let mut inner = Inner { src: self.src, root: ident(node), lines: Default::default() };
        inner.visit(node);
        // ponytail: RuboCop indexes `processed_source[line_number]` with the 1-based line number,
        // so it checks the line after each body line; copied as is.
        (first + 1..last)
            .filter(|l| !inner.lines.contains(l))
            .filter(|&l| self.src.lines.get(l).is_some_and(|s| !self.irrelevant(s)))
            .count()
    }
}

/// Lines of class/module nodes nested anywhere in a class/module.
struct Inner<'s> {
    src: &'s Source<'s>,
    root: (usize, usize, usize),
    lines: std::collections::HashSet<usize>,
}

impl Inner<'_> {
    fn add(&mut self, node: &Node) {
        if ident(node) != self.root {
            let l = node.location();
            self.lines.extend(self.src.line_of(l.start_offset())..=self.src.line_of(l.end_offset()));
        }
    }
}

impl<'pr> Visit<'pr> for Inner<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        self.add(&node.as_node());
        ruby_prism::visit_class_node(self, node);
    }
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'pr>) {
        self.add(&node.as_node());
        ruby_prism::visit_module_node(self, node);
    }
}

/// `node_with_heredoc?` and the last line of `source_from_node_with_heredoc` over parser descendants.
struct Descendants<'s> {
    src: &'s Source<'s>,
    root: Option<(usize, usize, usize)>,
    max_line: usize,
    /// A str/dstr heredoc descendant.
    heredoc: bool,
}

impl Descendants<'_> {
    fn record(&mut self, node: &Node) {
        if self.root == Some(ident(node)) {
            return;
        }
        let k = kind::of(node);
        if let Some(line) = heredoc_end_line(self.src, node).filter(|_| is_heredoc_node(node)) {
            self.heredoc |= matches!(k, kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE);
            self.max_line = self.max_line.max(line);
            return;
        }
        // Prism-only wrappers, or nodes whose Prism range runs past their parser range (to `end`).
        let transparent = match k {
            kind::STATEMENTS_NODE
            | kind::ARGUMENTS_NODE
            | kind::ELSE_NODE
            | kind::ENSURE_NODE
            | kind::BLOCK_NODE
            | kind::RESCUE_NODE
            | kind::PARAMETERS_NODE => true,
            kind::BEGIN_NODE => node.as_begin_node().unwrap().begin_keyword_loc().is_none(),
            kind::IF_NODE => node.as_if_node().unwrap().if_keyword_loc().is_some_and(|l| l.as_slice() == b"elsif"),
            _ => false,
        };
        if !transparent {
            self.max_line = self.max_line.max(self.src.line_of(node.location().end_offset()));
        }
    }
}

impl<'pr> Visit<'pr> for Descendants<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.record(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.record(&node);
    }
    // Typed children that Prism visits without the enter hooks.
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        self.record(&node.as_node());
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_block_argument_node(&mut self, node: &ruby_prism::BlockArgumentNode<'pr>) {
        self.record(&node.as_node());
        ruby_prism::visit_block_argument_node(self, node);
    }
    fn visit_splat_node(&mut self, node: &ruby_prism::SplatNode<'pr>) {
        self.record(&node.as_node());
        ruby_prism::visit_splat_node(self, node);
    }
    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode<'pr>) {
        self.record(&node.as_node());
        ruby_prism::visit_constant_path_node(self, node);
    }
    fn visit_block_parameter_node(&mut self, node: &ruby_prism::BlockParameterNode<'pr>) {
        self.record(&node.as_node());
    }
}

/// `CountAsOne`: folds the top-level array / hash / heredoc / method call descendants.
struct Folder<'c, 's> {
    calc: &'c Calc<'s>,
    folds: Folds,
    root: (usize, usize, usize),
    delta: i64,
}

impl Folder<'_, '_> {
    /// Folds the `send` on the left of `a.b ||= c` / `a[i] += c` when method calls fold.
    fn lhs_send<'pr>(&mut self, receiver: Option<Node<'pr>>, args: Option<Node<'pr>>, end: Option<usize>) -> bool {
        let (Some(recv), Some(end), true) = (receiver, end, self.folds.call) else { return false };
        let start = recv.location().start_offset();
        let mut parts = vec![recv];
        parts.extend(args);
        let length = self.calc.view_length(&View::Parts { range: (start, end), parts });
        self.fold(length, 0);
        true
    }

    fn fold(&mut self, length: usize, omit: usize) {
        self.delta -= length as i64 - 1 + omit as i64;
    }

    fn fold_node(&mut self, node: &Node) {
        let length = if is_heredoc_node(node) {
            self.calc.heredoc_length(node)
        } else {
            let l = node.location();
            self.calc.view_length(&View::Node { range: (l.start_offset(), l.end_offset()), node })
        };
        self.fold(length, 0);
    }

    /// A call's arguments: an unbraced hash argument may also drop its parentheses' lines.
    fn visit_args(&mut self, call: &CallNode) {
        let Some(args) = call.arguments() else { return };
        let nargs = crate::ast::send_args(call).len();
        for arg in args.arguments().iter() {
            if self.folds.hash && arg.as_keyword_hash_node().is_some() {
                let l = arg.location();
                let length =
                    self.calc.view_length(&View::Node { range: (l.start_offset(), l.end_offset()), node: &arg });
                let omit = match (call.opening_loc(), call.closing_loc()) {
                    (Some(o), Some(c)) if nargs <= 1 && o.as_slice() == b"(" => {
                        (o.end_offset() != l.start_offset()) as usize + (c.start_offset() != l.end_offset()) as usize
                    }
                    _ => 0,
                };
                self.fold(length, omit);
            } else {
                self.visit(&arg);
            }
        }
    }
}

impl<'pr> Visit<'pr> for Folder<'_, '_> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode<'pr>) {
        // parser wraps the exception list in an `array`
        let ex: Vec<Node> = node.exceptions().iter().collect();
        if self.folds.array && !ex.is_empty() {
            let range = (ex[0].location().start_offset(), ex[ex.len() - 1].location().end_offset());
            let length = self.calc.view_length(&View::Parts { range, parts: ex });
            self.fold(length, 0);
            if let Some(r) = node.reference() {
                self.visit(&r);
            }
            if let Some(s) = node.statements() {
                self.visit_statements_node(&s);
            }
            if let Some(n) = node.subsequent() {
                self.visit_rescue_node(&n);
            }
            return;
        }
        ruby_prism::visit_rescue_node(self, node);
    }
    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode<'pr>) {
        if !self.lhs_send(node.receiver(), None, node.message_loc().map(|l| l.end_offset())) {
            return ruby_prism::visit_call_or_write_node(self, node);
        }
        self.visit(&node.value());
    }
    fn visit_call_and_write_node(&mut self, node: &ruby_prism::CallAndWriteNode<'pr>) {
        if !self.lhs_send(node.receiver(), None, node.message_loc().map(|l| l.end_offset())) {
            return ruby_prism::visit_call_and_write_node(self, node);
        }
        self.visit(&node.value());
    }
    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode<'pr>) {
        if !self.lhs_send(node.receiver(), None, node.message_loc().map(|l| l.end_offset())) {
            return ruby_prism::visit_call_operator_write_node(self, node);
        }
        self.visit(&node.value());
    }
    fn visit_index_or_write_node(&mut self, node: &ruby_prism::IndexOrWriteNode<'pr>) {
        let args = node.arguments().map(|a| a.as_node());
        if !self.lhs_send(node.receiver(), args, Some(node.closing_loc().end_offset())) {
            return ruby_prism::visit_index_or_write_node(self, node);
        }
        self.visit(&node.value());
    }
    fn visit_index_and_write_node(&mut self, node: &ruby_prism::IndexAndWriteNode<'pr>) {
        let args = node.arguments().map(|a| a.as_node());
        if !self.lhs_send(node.receiver(), args, Some(node.closing_loc().end_offset())) {
            return ruby_prism::visit_index_and_write_node(self, node);
        }
        self.visit(&node.value());
    }
    fn visit_index_operator_write_node(&mut self, node: &ruby_prism::IndexOperatorWriteNode<'pr>) {
        let args = node.arguments().map(|a| a.as_node());
        if !self.lhs_send(node.receiver(), args, Some(node.closing_loc().end_offset())) {
            return ruby_prism::visit_index_operator_write_node(self, node);
        }
        self.visit(&node.value());
    }
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        let regexp_match = node.name().as_slice() == b"=~"
            && node.receiver().is_some_and(|r| r.as_regular_expression_node().is_some());
        let block = node.block().filter(|b| b.as_block_node().is_some());
        if regexp_match {
            return ruby_prism::visit_call_node(self, node);
        }
        if block.is_none() && self.folds.call && ident(&node.as_node()) != self.root {
            return self.fold_node(&node.as_node());
        }
        if block.is_some() && self.folds.call {
            let mut parts: Vec<Node> = node.receiver().into_iter().collect();
            parts.extend(node.arguments().map(|a| a.as_node()));
            parts.extend(node.block().filter(|b| b.as_block_argument_node().is_some()));
            let length = self.calc.view_length(&View::Parts { range: send_range(node), parts });
            self.fold(length, 0);
        } else {
            if let Some(r) = node.receiver() {
                self.visit(&r);
            }
            self.visit_args(node);
            if let Some(b) = node.block().filter(|b| b.as_block_argument_node().is_some()) {
                self.visit(&b);
            }
        }
        if let Some(b) = block {
            self.visit(&b);
        }
    }
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode<'pr>) {
        if self.folds.array && ident(&node.as_node()) != self.root {
            return self.fold_node(&node.as_node());
        }
        ruby_prism::visit_array_node(self, node);
    }
    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode<'pr>) {
        if self.folds.hash && ident(&node.as_node()) != self.root {
            return self.fold_node(&node.as_node());
        }
        ruby_prism::visit_hash_node(self, node);
    }
    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode<'pr>) {
        if self.folds.hash && ident(&node.as_node()) != self.root {
            return self.fold_node(&node.as_node());
        }
        ruby_prism::visit_keyword_hash_node(self, node);
    }
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        if self.folds.heredoc {
            if is_heredoc_node(&node.as_node()) {
                self.fold_node(&node.as_node());
            }
            return;
        }
        ruby_prism::visit_string_node(self, node);
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        if self.folds.heredoc {
            if is_heredoc_node(&node.as_node()) {
                self.fold_node(&node.as_node());
            }
            return;
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        if ident(&node.as_node()) == self.root {
            ruby_prism::visit_class_node(self, node);
        }
    }
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'pr>) {
        if ident(&node.as_node()) == self.root {
            ruby_prism::visit_module_node(self, node);
        }
    }
}

/// `(const {nil? cbase} name)`
pub fn global_const(node: &Node, names: &[&str]) -> bool {
    let name = if let Some(c) = node.as_constant_read_node() {
        c.name()
    } else if let Some(p) = node.as_constant_path_node().filter(|p| p.parent().is_none()) {
        match p.name() {
            Some(n) => n,
            None => return false,
        }
    } else {
        return false;
    };
    names.iter().any(|n| n.as_bytes() == name.as_slice())
}
