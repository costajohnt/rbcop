use crate::prelude::*;
use ruby_prism::StatementsNode;

pub const NAME: &str = "Lint/DuplicateBranch";
pub static COP: DuplicateBranch = DuplicateBranch;
pub struct DuplicateBranch;

const MSG: &str = "Duplicate branch body detected.";

/// One non-nil entry of rubocop-ast `node.branches`.
struct Branch<'pr> {
    /// Parser range of the branch body (one statement, or the `begin` of several).
    range: (usize, usize),
    /// The single statement (None for a multi-statement `begin`).
    node: Option<Node<'pr>>,
    /// `offense_range(branch)`.
    offense: (usize, usize),
    /// `branch.parent.respond_to?(:else?) && branch.parent.else?`
    parent_else: bool,
}

fn loc_range(l: &ruby_prism::Location) -> (usize, usize) {
    (l.start_offset(), l.end_offset())
}

fn push<'pr>(out: &mut Vec<Branch<'pr>>, body: Option<StatementsNode<'pr>>, offense: (usize, usize), parent_else: bool) {
    let Some(s) = body else { return };
    let stmts: Vec<Node> = s.body().iter().collect();
    let node = match stmts.len() {
        0 => return,
        1 => stmts.into_iter().next(),
        _ => None,
    };
    let range = match &node {
        Some(n) => loc_range(&n.location()),
        None => loc_range(&s.location()),
    };
    out.push(Branch { range, node, offense, parent_else });
}

/// End of a clause: its body, or else `fallback`.
fn body_end(body: &Option<StatementsNode>, fallback: usize) -> usize {
    body.as_ref().map_or(fallback, |s| s.location().end_offset())
}

/// `IfNode#branches` of an if/elsif chain (elsif branches flattened).
fn if_branches<'pr>(node: &Node<'pr>, out: &mut Vec<Branch<'pr>>) {
    let n = node.as_if_node().unwrap();
    let l = node.location();
    let own = (l.start_offset(), helpers::if_node::parser_end(node));
    if n.if_keyword_loc().is_none() {
        // Ternary: [if_branch, else_branch]; the else branch reports its own range.
        push(out, n.statements(), own, false);
        if let Some(e) = n.subsequent().and_then(|s| s.as_else_node()) {
            let r = e.statements().map(|s| loc_range(&s.location())).unwrap_or_default();
            push(out, e.statements(), r, false);
        }
        return;
    }
    let has_else = n.subsequent().is_some();
    push(out, n.statements(), own, has_else);
    match n.subsequent() {
        Some(s) if s.as_if_node().is_some() => if_branches(&s, out),
        Some(s) => {
            let e = s.as_else_node().unwrap();
            push(out, e.statements(), loc_range(&e.else_keyword_loc()), true);
        }
        None => {}
    }
}

fn branches<'pr>(src: &crate::source::Source, node: &Node<'pr>) -> Vec<Branch<'pr>> {
    let mut out = Vec::new();
    match kind::of(node) {
        kind::IF_NODE => if_branches(node, &mut out),
        kind::UNLESS_NODE => {
            let n = node.as_unless_node().unwrap();
            push(&mut out, n.statements(), loc_range(&node.location()), n.else_clause().is_some());
            if let Some(e) = n.else_clause() {
                push(&mut out, e.statements(), loc_range(&e.else_keyword_loc()), true);
            }
        }
        kind::CASE_NODE => {
            let n = node.as_case_node().unwrap();
            for w in n.conditions().iter() {
                let w = w.as_when_node().unwrap();
                let start = w.keyword_loc().start_offset();
                push(&mut out, w.statements(), (start, body_end(&w.statements(), start)), false);
            }
            if let Some(e) = n.else_clause() {
                push(&mut out, e.statements(), loc_range(&e.else_keyword_loc()), true);
            }
        }
        kind::CASE_MATCH_NODE => {
            let n = node.as_case_match_node().unwrap();
            for i in n.conditions().iter() {
                let i = i.as_in_node().unwrap();
                let start = i.in_loc().start_offset();
                push(&mut out, i.statements(), (start, body_end(&i.statements(), start)), false);
            }
            if let Some(e) = n.else_clause() {
                push(&mut out, e.statements(), loc_range(&e.else_keyword_loc()), true);
            }
        }
        kind::BEGIN_NODE => {
            let n = node.as_begin_node().unwrap();
            let mut rescue = n.rescue_clause();
            while let Some(r) = rescue {
                push(&mut out, r.statements(), helpers::resbody::range(src, &r), false);
                rescue = r.subsequent();
            }
            if let Some(e) = n.else_clause() {
                push(&mut out, e.statements(), loc_range(&e.else_keyword_loc()), true);
            }
        }
        _ => {}
    }
    out
}

fn is_const(node: &Node) -> bool {
    matches!(kind::of(node), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE)
}

/// Parser children of a literal (enough of them for `each_descendant` on literal trees), or None
/// when the node has a child that is never a basic literal, pair or const (cbase).
fn literal_children<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    Some(match node {
        Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().iter().collect(),
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().collect(),
        Node::AssocNode { .. } => {
            let a = node.as_assoc_node().unwrap();
            vec![a.key(), a.value()]
        }
        Node::RangeNode { .. } => {
            let r = node.as_range_node().unwrap();
            r.left().into_iter().chain(r.right()).collect()
        }
        Node::InterpolatedStringNode { .. } => node.as_interpolated_string_node().unwrap().parts().iter().collect(),
        Node::InterpolatedSymbolNode { .. } => node.as_interpolated_symbol_node().unwrap().parts().iter().collect(),
        // regexp: str parts plus a `regopt`, which is a basic literal.
        Node::InterpolatedRegularExpressionNode { .. } => {
            node.as_interpolated_regular_expression_node().unwrap().parts().iter().collect()
        }
        Node::ConstantPathNode { .. } => node.as_constant_path_node().unwrap().parent().map(|p| vec![p])?,
        // A multi-line str (parser dstr) holds one str per line; other leaves have no node children.
        _ => Vec::new(),
    })
}

fn all_descendants_literal(node: &Node, ignore_const: bool) -> bool {
    let Some(children) = literal_children(node) else { return false };
    // Parts of an interpolated string are one parser `str` per line.
    let parts = matches!(
        node,
        Node::InterpolatedStringNode { .. } | Node::InterpolatedSymbolNode { .. } | Node::InterpolatedRegularExpressionNode { .. }
    );
    children.iter().all(|c| {
        (helpers::literal_node::basic_literal(c)
            || matches!(c, Node::AssocNode { .. })
            || (is_const(c) && ignore_const)
            || (parts && matches!(c, Node::StringNode { .. })))
            && all_descendants_literal(c, ignore_const)
    })
}

fn literal_branch(node: &Node, ignore_const: bool) -> bool {
    if !helpers::literal_node::literal(node)
        || matches!(node, Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. })
    {
        return false;
    }
    helpers::literal_node::basic_literal(node) || all_descendants_literal(node, ignore_const)
}

impl Cop for DuplicateBranch {
    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::CASE_NODE, kind::CASE_MATCH_NODE, kind::BEGIN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(n) = node.as_if_node()
            && (is_pattern_guard(node, cx.ancestors) || n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif"))
        {
            return;
        }
        if node.as_begin_node().is_some_and(|b| b.rescue_clause().is_none()) {
            return;
        }
        let ignore_literal = cx.cfg.bool("IgnoreLiteralBranches").unwrap_or(false);
        let ignore_const = cx.cfg.bool("IgnoreConstantBranches").unwrap_or(false);
        let ignore_else = cx.cfg.bool("IgnoreDuplicateElseBranch").unwrap_or(false);
        let branches = branches(cx.src, node);
        let mut previous: Vec<Vec<String>> = Vec::new();
        for (i, b) in branches.iter().enumerate() {
            if ignore_literal && b.node.as_ref().is_some_and(|n| literal_branch(n, ignore_const)) {
                continue;
            }
            if ignore_const && b.node.as_ref().is_some_and(is_const) {
                continue;
            }
            if ignore_else && branches.len() > 2 && i == branches.len() - 1 && b.parent_else {
                continue;
            }
            let shape = helpers::node_equality::hash_shape(cx.src, b.range.0, b.range.1);
            if previous.contains(&shape) {
                cx.add(b.offense.0, b.offense.1, MSG);
            } else {
                previous.push(shape);
            }
        }
    }
}
