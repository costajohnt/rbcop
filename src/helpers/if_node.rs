//! Parser-gem shapes of `if` nodes that Prism represents differently.

use crate::ast::method_name;
use crate::node_kind as kind;
use crate::source::Source;
use ruby_prism::Node;

/// End of parser's `if` range: `end`, or for an `elsif` (no `end` in parser) its last part
/// (Parser::Builders::Default#condition_map: else body, else keyword, body, `then`, condition).
pub fn parser_end(node: &Node) -> usize {
    let Some(n) = node.as_if_node() else { return node.location().end_offset() };
    if n.if_keyword_loc().is_some_and(|k| k.as_slice() != b"elsif") {
        return node.location().end_offset();
    }
    match n.subsequent() {
        Some(s) => match s.as_else_node() {
            Some(e) => e.statements().map_or(e.else_keyword_loc().end_offset(), |s| s.location().end_offset()),
            None => parser_end(&s),
        },
        None => match (n.statements(), then_keyword(&n)) {
            (Some(s), _) => s.location().end_offset(),
            (None, Some(t)) => t.end_offset(),
            (None, None) => {
                // A `;` terminating the condition is parser's `then` token (loc.begin).
                let end = n.predicate().location().end_offset();
                let rest = &node.location().as_slice()[end - node.location().start_offset()..];
                let blanks = rest.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
                if rest.get(blanks) == Some(&b';') { end + blanks + 1 } else { end }
            }
        },
    }
}

/// An if node's `then` (or `;`): Prism gives an empty `elsif b;end` the `then` of the `if` it
/// follows (#636), before the condition.
// ponytail: only parser_end and Lint/EmptyConditionalBody use it; other then_keyword_loc readers
// see Prism's value.
pub fn then_keyword<'pr>(n: &ruby_prism::IfNode<'pr>) -> Option<ruby_prism::Location<'pr>> {
    n.then_keyword_loc().filter(|t| t.start_offset() >= n.predicate().location().end_offset())
}

/// OnNormalIfUnless: the keyword (`if`, `elsif`, `unless`) of an if/unless node that is neither a
/// modifier nor a ternary; None otherwise. Pattern guards must be skipped by the caller.
pub fn normal_keyword(node: &Node) -> Option<&'static str> {
    if let Some(n) = node.as_if_node() {
        let kw = n.if_keyword_loc()?;
        if kw.as_slice() == b"elsif" {
            return Some("elsif");
        }
        return n.end_keyword_loc().is_some().then_some("if");
    }
    node.as_unless_node().filter(|n| n.end_keyword_loc().is_some()).map(|_| "unless")
}

/// A non-empty parser branch body (`if_branch`, `else_branch`, a `when` body): one statement, or a
/// parser `begin` of 2+ statements.
pub struct Body<'pr> {
    pub stmts: ruby_prism::StatementsNode<'pr>,
}

impl<'pr> Body<'pr> {
    pub fn new(s: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Self> {
        s.filter(|s| s.body().iter().next().is_some()).map(|stmts| Body { stmts })
    }
    /// The parser node when it is a single statement.
    pub fn single(&self) -> Option<Node<'pr>> {
        let mut it = self.stmts.body().iter();
        let first = it.next();
        if it.next().is_some() { None } else { first }
    }
    /// `begin_type?`: 2+ statements or a parenthesised expression.
    pub fn is_begin(&self) -> bool {
        self.single().is_none_or(|n| n.as_parentheses_node().is_some())
    }
    /// `branch.begin_type? ? Array(branch).last : branch` (None for an empty `()`).
    pub fn tail(&self) -> Option<Node<'pr>> {
        let Some(n) = self.single() else { return self.stmts.body().iter().last() };
        let Some(p) = n.as_parentheses_node() else { return Some(n) };
        let body = p.body()?;
        match body.as_statements_node() {
            Some(s) => s.body().iter().last(),
            None => Some(body),
        }
    }
    pub fn range(&self) -> (usize, usize) {
        (self.stmts.location().start_offset(), self.stmts.location().end_offset())
    }
}

/// The else part of a parser if node: an `elsif`, or an `else` clause with its body.
pub enum Else<'pr> {
    Elsif(Node<'pr>),
    Body(Option<Body<'pr>>),
}

/// rubocop-ast IfNode parts of an if/unless (any form): `condition`, `if_branch` (unless: its
/// body), and the else part (None without `else`/`elsif`).
pub fn parts<'pr>(node: &Node<'pr>) -> (Node<'pr>, Option<Body<'pr>>, Option<Else<'pr>>) {
    if let Some(n) = node.as_if_node() {
        let els = n.subsequent().map(|s| match s.as_else_node() {
            Some(e) => Else::Body(Body::new(e.statements())),
            None => Else::Elsif(s),
        });
        return (n.predicate(), Body::new(n.statements()), els);
    }
    let n = node.as_unless_node().unwrap();
    (n.predicate(), Body::new(n.statements()), n.else_clause().map(|e| Else::Body(Body::new(e.statements()))))
}

/// `node.elsif?`
pub fn is_elsif(node: &Node) -> bool {
    node.as_if_node().is_some_and(|n| n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif"))
}

/// End of parser's `loc.begin` of an if/unless: `then`, or a `;` right after the condition
/// (blanks and newlines between them allowed: `if a\n; b end` has `;` as loc.begin).
pub fn parser_begin_end(node: &Node) -> Option<usize> {
    let (then, pred) = match (node.as_if_node(), node.as_unless_node()) {
        (Some(n), _) => (n.then_keyword_loc(), n.predicate()),
        (_, Some(n)) => (n.then_keyword_loc(), n.predicate()),
        _ => return None,
    };
    if let Some(t) = then {
        return Some(t.end_offset());
    }
    let end = pred.location().end_offset();
    let rest = &node.location().as_slice()[end - node.location().start_offset()..];
    let blanks = rest.iter().take_while(|&&b| crate::helpers::parser_tokens::is_ruby_space(b)).count();
    (rest.get(blanks) == Some(&b';')).then_some(end + blanks + 1)
}

/// rubocop-ast IfNode view of a Prism `IfNode` / `UnlessNode`.
pub struct Cond<'pr> {
    /// `if`, `unless`, `elsif`, or `""` for a ternary.
    pub keyword: &'pr str,
    /// `loc.keyword` (None for a ternary).
    pub keyword_loc: Option<(usize, usize)>,
    pub condition: Node<'pr>,
    /// rubocop-ast `body` / `if_branch`: the single statement, or the StatementsNode (parser
    /// `begin`) when there are several; None when empty.
    pub body: Option<Node<'pr>>,
    /// `loc.end` is present.
    pub has_end: bool,
    /// `loc.else` is present (an `else` or `elsif` follows).
    pub has_else: bool,
}

impl Cond<'_> {
    /// `modifier_form?`
    pub fn modifier(&self) -> bool {
        matches!(self.keyword, "if" | "unless") && !self.has_end
    }
    pub fn ternary(&self) -> bool {
        self.keyword.is_empty()
    }
}

/// `parts` plus the keyword facts of an if/unless (any form).
pub fn cond<'pr>(node: &Node<'pr>) -> Option<Cond<'pr>> {
    let (keyword, keyword_loc, end) = match node {
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            let kw = n.if_keyword_loc();
            let keyword = kw.as_ref().map_or("", |k| std::str::from_utf8(k.as_slice()).unwrap_or(""));
            (keyword, kw.map(|k| (k.start_offset(), k.end_offset())), n.end_keyword_loc().is_some())
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            let kw = n.keyword_loc();
            ("unless", Some((kw.start_offset(), kw.end_offset())), n.end_keyword_loc().is_some())
        }
        _ => return None,
    };
    let (condition, body, els) = parts(node);
    Some(Cond {
        keyword,
        keyword_loc,
        condition,
        body: body.map(|b| b.single().unwrap_or_else(|| b.stmts.as_node())),
        // An `elsif` has no `loc.end` in parser although Prism gives it the outer `end`.
        has_end: end && keyword != "elsif",
        has_else: els.is_some(),
    })
}

/// IfNode#if_branch when it is a single statement (a `begin` is never a guard clause).
pub fn if_branch<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let stmts = match kind::of(node) {
        kind::IF_NODE => node.as_if_node().unwrap().statements(),
        kind::UNLESS_NODE => node.as_unless_node().unwrap().statements(),
        _ => None,
    }?;
    let body: Vec<Node> = stmts.body().iter().collect();
    match body.len() {
        1 => body.into_iter().next(),
        _ => None,
    }
}

/// `{(send nil? {:raise :fail} ...) return break next}`
pub fn guard_clause_branch(node: &Node) -> bool {
    match kind::of(node) {
        kind::RETURN_NODE | kind::BREAK_NODE | kind::NEXT_NODE => true,
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            c.receiver().is_none()
                && matches!(method_name(&c), "raise" | "fail")
                && c.block().is_none_or(|b| b.as_block_node().is_none())
        }
        _ => false,
    }
}

/// Node#guard_clause?: the rhs of and/or, matching the pattern and on a single line.
pub fn guard_clause(src: &Source, node: &Node) -> bool {
    let node = match kind::of(node) {
        kind::AND_NODE => node.as_and_node().unwrap().right(),
        kind::OR_NODE => node.as_or_node().unwrap().right(),
        _ => return guard_clause_inner(src, node),
    };
    guard_clause_inner(src, &node)
}

fn guard_clause_inner(src: &Source, node: &Node) -> bool {
    let loc = node.location();
    guard_clause_branch(node) && src.line_of(loc.start_offset()) == src.line_of(loc.end_offset())
}
