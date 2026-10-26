use crate::prelude::*;
use ruby_prism::{ReturnNode, Visit};

pub const NAME: &str = "Style/ReturnNilInPredicateMethodDefinition";
pub static COP: ReturnNilInPredicateMethodDefinition = ReturnNilInPredicateMethodDefinition;
pub struct ReturnNilInPredicateMethodDefinition;

const MSG: &str = "Return `false` instead of `nil` in predicate methods.";

/// A Prism node as parser sees it: the single statement, or a `begin` with its children.
enum Parsed<'pr> {
    Node(Node<'pr>),
    Begin(Vec<Node<'pr>>),
}

fn parsed<'pr>(node: Node<'pr>) -> Parsed<'pr> {
    if let Some(s) = node.as_statements_node() {
        let mut v: Vec<Node> = s.body().iter().collect();
        return if v.len() == 1 { Parsed::Node(v.pop().unwrap()) } else { Parsed::Begin(v) };
    }
    if let Some(p) = node.as_parentheses_node() {
        let v = match p.body() {
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![b],
            },
            None => Vec::new(),
        };
        return Parsed::Begin(v);
    }
    Parsed::Node(node)
}

/// `last_node_of_type`'s candidate: the parser node itself, or the last child of a `begin`.
fn candidate(node: Node) -> Option<Node> {
    let n = match parsed(node) {
        Parsed::Begin(mut v) => return v.pop(),
        Parsed::Node(n) => n,
    };
    match parsed(n) {
        Parsed::Begin(mut v) => v.pop(),
        Parsed::Node(n) => Some(n),
    }
}

/// The `return nil` / `return` nodes, and the start offsets of the returns whose parser parent is
/// a `&&` / `||` operation or a ternary (`requires_parentheses?`).
#[derive(Default)]
struct Returns(Vec<(usize, usize)>, Vec<usize>);

impl Returns {
    fn operands<'pr>(&mut self, parent: &Node<'pr>, children: impl IntoIterator<Item = Option<Node<'pr>>>) {
        if helpers::parser_parent::return_needs_parentheses(parent) {
            for c in children.into_iter().flatten() {
                // A ternary branch is a single statement.
                let c = c.as_statements_node().and_then(|s| s.body().iter().next()).unwrap_or(c);
                if c.as_return_node().is_some() {
                    self.1.push(c.location().start_offset());
                }
            }
        }
    }
}

impl<'pr> Visit<'pr> for Returns {
    fn visit_return_node(&mut self, node: &ReturnNode<'pr>) {
        // `{(return) (return (nil))}`
        let args: Vec<Node> = node.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        if matches!(args.as_slice(), [] | [Node::NilNode { .. }]) {
            let l = node.location();
            self.0.push((l.start_offset(), l.end_offset()));
        }
        ruby_prism::visit_return_node(self, node);
    }
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'pr>) {
        self.operands(&node.as_node(), [Some(node.left()), Some(node.right())]);
        ruby_prism::visit_and_node(self, node);
    }
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode<'pr>) {
        self.operands(&node.as_node(), [Some(node.left()), Some(node.right())]);
        ruby_prism::visit_or_node(self, node);
    }
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        let els = node.subsequent().and_then(|s| s.as_else_node()).and_then(|e| e.statements()).map(|s| s.as_node());
        self.operands(&node.as_node(), [node.statements().map(|s| s.as_node()), els]);
        ruby_prism::visit_if_node(self, node);
    }
}

/// `handle_implicit_return_values`.
fn implicit(cx: &mut Cx, node: Option<Node>) {
    let Some(n) = node.and_then(candidate) else { return };
    if let Some(i) = n.as_if_node() {
        let els = i.subsequent().and_then(|s| match s.as_else_node() {
            Some(e) => e.statements().map(|s| s.as_node()),
            None => Some(s),
        });
        implicit(cx, i.statements().map(|s| s.as_node()));
        implicit(cx, els);
    } else if let Some(u) = n.as_unless_node() {
        implicit(cx, u.statements().map(|s| s.as_node()));
        implicit(cx, u.else_clause().and_then(|e| e.statements()).map(|s| s.as_node()));
    } else if n.as_nil_node().is_some() {
        let l = n.location();
        let (s, e) = (l.start_offset(), l.end_offset());
        cx.add_fix(s, e, MSG, |c| c.replace(s, e, "false"));
    }
}

impl Cop for ReturnNilInPredicateMethodDefinition {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
        if !name.ends_with('?') {
            return;
        }
        if helpers::allowed::allowed_method(cx.cfg, name) || helpers::allowed::matches_allowed_pattern(cx.cfg, name) {
            return;
        }
        let Some(body) = def.body() else { return };
        // `body.each_descendant(:return)`: the parser body itself is excluded.
        let mut v = Returns::default();
        match parsed(body) {
            Parsed::Node(n) => match n.as_return_node() {
                Some(r) => ruby_prism::visit_return_node(&mut v, &r),
                None => v.visit(&n),
            },
            Parsed::Begin(stmts) => stmts.iter().for_each(|n| v.visit(n)),
        }
        for (s, e) in v.0 {
            let replacement = if v.1.contains(&s) { "(return false)" } else { "return false" };
            cx.add_fix(s, e, MSG, |c| c.replace(s, e, replacement));
        }
        // Below 2.6 the statements after a lonely `else` are a `begin` of their own: the last
        // value is that `begin`, never `nil` (#630).
        if let Some(s) = def.body().and_then(|b| b.as_statements_node()) {
            let stmts: Vec<Node> = s.body().iter().collect();
            if stmts.last().is_some_and(|l| cx.src.after_lonely_else(def.location().start_offset(), &stmts, l)) {
                return;
            }
        }
        implicit(cx, def.body());
    }
}
