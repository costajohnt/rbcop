use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/SkipEnsure";
pub static COP: SkipEnsure = SkipEnsure;
pub struct SkipEnsure;

impl Cop for SkipEnsure {
    fn kinds(&self) -> &'static [usize] {
        &[kind::BEGIN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let begin = node.as_begin_node().unwrap();
        let Some(ensure) = begin.ensure_clause() else { return };
        // With a rescue clause the ensure body is a parser `rescue`, an ancestor of any skip in it.
        if begin.rescue_clause().is_some() {
            return;
        }
        let Some(stmts) = begin.statements() else { return };
        let body = stmts.as_node();
        // body.descendants: a single-statement body is not its own descendant.
        let mut f = FindSkip { stack: Vec::new(), found: None, root: range(&body), skip_root: !is_begin(&body) };
        ruby_prism::Visit::visit(&mut f, &body);
        let Some((rescue_inside, if_inside)) = f.found else { return };
        if rescue_inside || outer_rescue(cx.ancestors, node) {
            return;
        }
        let ensure_body = parser_body(ensure.statements().map(|s| s.as_node()));
        if let Some((branch_kw, branch_cond)) = ensure_body.as_ref().and_then(if_parts) {
            // the skip's nearest if ancestor (none raises: no offense)
            let skip_if = if_inside.or_else(|| cx.ancestors.iter().rev().find_map(if_parts));
            let Some((kw, cond)) = skip_if else { return };
            let matched = if branch_kw == "if" { kw == "if" } else { kw == "unless" };
            if matched && helpers::node_equality::same(cx.src, branch_cond, cond) {
                return;
            }
        }
        let k = ensure.ensure_keyword_loc();
        cx.add(k.start_offset(), k.end_offset(), "`ensure` is called even though the test is skipped.");
    }
}

/// A parser `if` (if/unless/ternary/modifier): its keyword ("if", "unless" or "" for a ternary)
/// and condition range.
fn if_parts(node: &Node) -> Option<(&'static str, (usize, usize))> {
    match node {
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            let kw = if n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") { "if" } else { "" };
            Some((kw, range(&n.predicate())))
        }
        Node::UnlessNode { .. } => Some(("unless", range(&node.as_unless_node().unwrap().predicate()))),
        _ => None,
    }
}

/// A rescue ancestor of the ensure (parser `rescue`: a begin/def body with rescue clauses around
/// it, a rescue clause, or a rescue modifier).
fn outer_rescue(ancestors: &[Node], node: &Node) -> bool {
    let me = range(node);
    ancestors.iter().any(|a| match a {
        Node::RescueNode { .. } | Node::RescueModifierNode { .. } => true,
        Node::BeginNode { .. } => {
            let b = a.as_begin_node().unwrap();
            b.rescue_clause().is_some()
                && (b.statements().is_some_and(|s| {
                    let r = range(&s.as_node());
                    r.0 <= me.0 && me.1 <= r.1
                }) || b.else_clause().is_some_and(|e| {
                    let r = range(&e.as_node());
                    r.0 <= me.0 && me.1 <= r.1
                }))
        }
        _ => false,
    })
}

/// The first `skip` send (no receiver) in pre-order, with whether a rescue lies between it and
/// the walk's root and its nearest if ancestor below the root.
/// (a rescue node, the node as a parser `if`: keyword and condition)
type Frame = (bool, Option<(&'static str, (usize, usize))>);

struct FindSkip {
    stack: Vec<Frame>,
    found: Option<Frame>,
    root: (usize, usize),
    skip_root: bool,
}

impl<'pr> ruby_prism::Visit<'pr> for FindSkip {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let rescue = matches!(node, Node::RescueNode { .. } | Node::RescueModifierNode { .. })
            || node.as_begin_node().is_some_and(|b| b.rescue_clause().is_some());
        let iff = if_parts(&node);
        self.stack.push((rescue, iff));
        if self.found.is_none()
            && let Some(c) = as_send(&node)
            && c.receiver().is_none()
            && method_name(&c) == "skip"
            && !(self.skip_root && range(&node) == self.root)
        {
            let rescue = self.stack[..self.stack.len() - 1].iter().any(|s| s.0);
            let iff = self.stack[..self.stack.len() - 1].iter().rev().find_map(|s| s.1);
            self.found = Some((rescue, iff));
        }
    }
    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        if self.found.is_none()
            && let Some(c) = as_send(&node)
            && c.receiver().is_none()
            && method_name(&c) == "skip"
            && !(self.skip_root && range(&node) == self.root)
        {
            let rescue = self.stack.iter().any(|s| s.0);
            let iff = self.stack.iter().rev().find_map(|s| s.1);
            self.found = Some((rescue, iff));
        }
    }
}
