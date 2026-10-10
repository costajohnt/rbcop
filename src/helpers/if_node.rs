//! Parser-gem shapes of `if` nodes that Prism represents differently.

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
        None => match (n.statements(), n.then_keyword_loc()) {
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
