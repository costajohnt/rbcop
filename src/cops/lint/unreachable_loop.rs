use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/UnreachableLoop";
pub static COP: UnreachableLoop = UnreachableLoop;
pub struct UnreachableLoop;

const MSG: &str = "This loop will have at most one iteration.";

/// `Enumerable.instance_methods + [:each]` (Ruby 4.0).
const ENUMERABLE_METHODS: &[&str] = &[
    "all?", "any?", "chain", "chunk", "chunk_while", "collect", "collect_concat", "compact", "count", "cycle",
    "detect", "drop", "drop_while", "each", "each_cons", "each_entry", "each_slice", "each_with_index",
    "each_with_object", "entries", "filter", "filter_map", "find", "find_all", "find_index", "first", "flat_map",
    "grep", "grep_v", "group_by", "include?", "inject", "lazy", "map", "max", "max_by", "member?", "min", "min_by",
    "minmax", "minmax_by", "none?", "one?", "partition", "reduce", "reject", "reverse_each", "select", "slice_after",
    "slice_before", "slice_when", "sort", "sort_by", "sum", "take", "take_while", "tally", "to_a", "to_h", "to_set",
    "uniq", "zip",
];

impl Cop for UnreachableLoop {
    fn kinds(&self) -> &'static [usize] {
        &[kind::WHILE_NODE, kind::UNTIL_NODE, kind::FOR_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (body, the loop's children before its body: `left_siblings` of a sole body statement;
        // `true` marks nodes that stand for their whole subtree, the block's send node's parts)
        let (body, prefix): (Option<Node>, Vec<(Node, bool)>) = match node {
            Node::WhileNode { .. } => {
                let w = node.as_while_node().unwrap();
                (w.statements().map(|s| s.as_node()), vec![(w.predicate(), false)])
            }
            Node::UntilNode { .. } => {
                let w = node.as_until_node().unwrap();
                (w.statements().map(|s| s.as_node()), vec![(w.predicate(), false)])
            }
            Node::ForNode { .. } => {
                let f = node.as_for_node().unwrap();
                (f.statements().map(|s| s.as_node()), vec![(f.index(), false), (f.collection(), false)])
            }
            _ => {
                let call = node.as_call_node().unwrap();
                let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
                if !loop_method(cx, node) {
                    return;
                }
                let mut prefix: Vec<(Node, bool)> = call.receiver().into_iter().map(|r| (r, true)).collect();
                prefix.extend(call.arguments().into_iter().flat_map(|a| a.arguments().iter()).map(|a| (a, true)));
                prefix.extend(block.parameters().map(|p| (p, true)));
                (block.body(), prefix)
            }
        };
        let (statements, single) = match body {
            None => (Vec::new(), false),
            Some(b) => match begin_children(&b) {
                Some(children) => (children, false),
                None => match b.as_statements_node() {
                    Some(s) => (s.body().iter().collect(), true),
                    None => (vec![b], true),
                },
            },
        };
        let Some(i) = statements.iter().position(|s| break_statement(cx, s)) else { return };
        let siblings: Vec<(&Node, bool)> = if single {
            prefix.iter().map(|(n, inclusive)| (n, *inclusive)).collect()
        } else {
            statements[..i].iter().map(|s| (s, false)).collect()
        };
        if !preceded_by_continue(cx, &siblings) && !conditional_continue_keyword(&statements[i]) {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), MSG);
        }
    }
}

/// Children of a parser `begin`: parentheses or a multi-statement body.
fn begin_children<'pr>(n: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    match n {
        Node::StatementsNode { .. } => {
            let body: Vec<Node> = n.as_statements_node().unwrap().body().iter().collect();
            if body.len() > 1 { Some(body) } else { None }
        }
        Node::ParenthesesNode { .. } => Some(match n.as_parentheses_node().unwrap().body() {
            None => Vec::new(),
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![b],
            },
        }),
        _ => None,
    }
}

/// A branch body's statements (one statement, or the children of a parser `begin`).
fn branch<'pr>(s: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Vec<Node<'pr>>> {
    s.map(|s| s.body().iter().collect())
}

fn branch_breaks(cx: &Cx, b: Option<Vec<Node>>) -> bool {
    match b {
        None => false,
        Some(v) if v.len() == 1 => break_statement(cx, &v[0]),
        Some(v) => begin_breaks(cx, &v),
    }
}

fn break_command(node: &Node) -> bool {
    match node {
        Node::ReturnNode { .. } | Node::BreakNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
                return false;
            }
            let receiver_ok = match call.receiver() {
                None => true,
                Some(r) => match &r {
                    Node::ConstantReadNode { .. } => node_text(&r) == "Kernel",
                    _ => r.as_constant_path_node().is_some_and(|p| {
                        p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Kernel")
                    }),
                },
            };
            receiver_ok && matches!(method_name(&call), "raise" | "fail" | "throw" | "exit" | "exit!" | "abort")
        }
        _ => false,
    }
}

fn break_statement(cx: &Cx, node: &Node) -> bool {
    if break_command(node) {
        return true;
    }
    if let Some(children) = begin_children(node) {
        return begin_breaks(cx, &children);
    }
    match node {
        Node::BeginNode { .. } => {
            let b = node.as_begin_node().unwrap();
            // kwbegin with rescue/ensure: its only child is the rescue/ensure node.
            if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                return false;
            }
            begin_breaks(cx, &branch(b.statements()).unwrap_or_default())
        }
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            let else_branch = match n.subsequent() {
                None => None,
                Some(s) => match s.as_else_node() {
                    Some(e) => branch(e.statements()),
                    None => Some(vec![s]),
                },
            };
            if_breaks(cx, branch(n.statements()), else_branch)
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            if_breaks(cx, branch(n.statements()), n.else_clause().and_then(|e| branch(e.statements())))
        }
        Node::CaseNode { .. } => {
            let n = node.as_case_node().unwrap();
            let bodies = n.conditions().iter().map(|w| branch(w.as_when_node().unwrap().statements())).collect();
            case_breaks(cx, n.else_clause().and_then(|e| branch(e.statements())), bodies)
        }
        Node::CaseMatchNode { .. } => {
            let n = node.as_case_match_node().unwrap();
            let bodies = n.conditions().iter().map(|w| branch(w.as_in_node().unwrap().statements())).collect();
            case_breaks(cx, n.else_clause().and_then(|e| branch(e.statements())), bodies)
        }
        _ => false,
    }
}

fn begin_breaks(cx: &Cx, children: &[Node]) -> bool {
    match children.iter().position(|s| break_statement(cx, s)) {
        None => false,
        Some(i) => {
            let siblings: Vec<(&Node, bool)> = children[..i].iter().map(|s| (s, false)).collect();
            !preceded_by_continue(cx, &siblings)
        }
    }
}

fn if_breaks(cx: &Cx, if_branch: Option<Vec<Node>>, else_branch: Option<Vec<Node>>) -> bool {
    if_branch.is_some() && else_branch.is_some() && branch_breaks(cx, if_branch) && branch_breaks(cx, else_branch)
}

fn case_breaks(cx: &Cx, else_branch: Option<Vec<Node>>, bodies: Vec<Option<Vec<Node>>>) -> bool {
    if else_branch.is_none() || !branch_breaks(cx, else_branch) {
        return false;
    }
    bodies.into_iter().all(|b| b.is_some() && branch_breaks(cx, b))
}

/// `loop_method?`: a block on an enumerable/enumerator method or `loop`, not an allowed pattern.
fn loop_method(cx: &Cx, node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if !call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    let name = method_name(&call);
    let loopable = ENUMERABLE_METHODS.contains(&name)
        || is_enumerator_method(name)
        || name == "loop";
    let (s, e) = send_range(&call);
    loopable && !helpers::allowed::matches_allowed_pattern(cx.cfg, &cx.src.text[s..e])
}

fn loop_keyword(node: &Node) -> bool {
    matches!(node, Node::WhileNode { .. } | Node::UntilNode { .. } | Node::ForNode { .. })
}

#[derive(Default)]
struct FindContinue {
    found: bool,
}

impl<'pr> Visit<'pr> for FindContinue {
    fn visit_next_node(&mut self, _: &ruby_prism::NextNode<'pr>) {
        self.found = true;
    }
    fn visit_redo_node(&mut self, _: &ruby_prism::RedoNode<'pr>) {
        self.found = true;
    }
}

/// `left_siblings.any? { each_descendant(:next, :redo).any? }`, skipping loops.
fn preceded_by_continue(cx: &Cx, siblings: &[(&Node, bool)]) -> bool {
    siblings.iter().any(|&(s, inclusive)| {
        if !inclusive && (loop_keyword(s) || loop_method(cx, s)) {
            return false;
        }
        if matches!(s, Node::NextNode { .. } | Node::RedoNode { .. }) {
            // each_descendant excludes the node itself.
            // ponytail: a `next` nested in a sibling `next`'s arguments is not found.
            return inclusive;
        }
        let mut v = FindContinue::default();
        v.visit(s);
        v.found
    })
}

#[derive(Default)]
struct LastOr {
    rhs_continue: Option<bool>,
}

impl<'pr> Visit<'pr> for LastOr {
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode<'pr>) {
        self.rhs_continue = Some(matches!(node.right(), Node::NextNode { .. } | Node::RedoNode { .. }));
        ruby_prism::visit_or_node(self, node);
    }
}

/// `break_statement.each_descendant(:or).to_a.last` has a `next`/`redo` right-hand side.
fn conditional_continue_keyword(statement: &Node) -> bool {
    let mut v = LastOr::default();
    v.visit(statement);
    v.rhs_continue == Some(true)
}
