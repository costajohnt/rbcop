use crate::prelude::*;
use std::cell::Cell;

pub const NAME: &str = "Style/IdenticalConditionalBranches";
pub static COP: IdenticalConditionalBranches = IdenticalConditionalBranches;
pub struct IdenticalConditionalBranches;

/// A parser branch body: nil, a `begin` (several statements or parentheses) or a single node.
enum Branch<'pr> {
    Nil,
    Begin(Vec<Node<'pr>>),
    One(Node<'pr>),
}

impl<'pr> Branch<'pr> {
    fn of(stmts: Option<ruby_prism::StatementsNode<'pr>>) -> Self {
        let body: Vec<Node> = stmts.map(|s| s.body().iter().collect()).unwrap_or_default();
        match body.len() {
            0 => Branch::Nil,
            1 => Branch::single(body.into_iter().next().unwrap()),
            _ => Branch::Begin(body),
        }
    }

    fn single(node: Node<'pr>) -> Self {
        let Some(p) = node.as_parentheses_node() else { return Branch::One(node) };
        Branch::Begin(match p.body() {
            None => Vec::new(),
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![b],
            },
        })
    }

    fn head(&self) -> Option<&Node<'pr>> {
        match self {
            Branch::Nil => None,
            Branch::Begin(v) => v.first(),
            Branch::One(n) => Some(n),
        }
    }

    fn tail(&self) -> Option<&Node<'pr>> {
        match self {
            Branch::Nil => None,
            Branch::Begin(v) => v.last(),
            Branch::One(n) => Some(n),
        }
    }

    fn single_child(&self) -> bool {
        match self {
            Branch::Begin(v) => v.len() == 1,
            _ => true,
        }
    }
}

/// Parser branches of an if-like node from its raw children `(cond, a, b)`; `expand_elses(b)`.
fn expand_elses<'pr>(branch: Branch<'pr>, out: &mut Vec<Branch<'pr>>) {
    if let Branch::One(n) = &branch {
        if let Some(i) = n.as_if_node() {
            out.push(Branch::of(i.statements()));
            expand_elses(else_of_if(&i), out);
            return;
        }
        if let Some(u) = n.as_unless_node() {
            // raw parser children of `unless`: (cond, else_part, body)
            out.push(Branch::of(u.else_clause().and_then(|e| e.statements())));
            expand_elses(Branch::of(u.statements()), out);
            return;
        }
    }
    out.push(branch);
}

fn else_of_if<'pr>(i: &ruby_prism::IfNode<'pr>) -> Branch<'pr> {
    match i.subsequent() {
        None => Branch::Nil,
        Some(s) => match s.as_else_node() {
            Some(e) => Branch::of(e.statements()),
            None => Branch::One(s),
        },
    }
}

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

fn is_variable(node: &Node) -> bool {
    matches!(
        node,
        Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
    )
}

/// Parser `child_nodes` of a condition (enough for `variable?` children).
fn condition_children<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    match node {
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            let mut v: Vec<Node> = c.receiver().into_iter().collect();
            v.extend(send_args(&c));
            v
        }
        Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            vec![n.left(), n.right()]
        }
        Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            vec![n.left(), n.right()]
        }
        Node::ParenthesesNode { .. } => match Branch::single(node.as_parentheses_node().unwrap().as_node()) {
            Branch::Begin(v) => v,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// `child_nodes.first` of an assignment node (the "lhs" RuboCop compares with the condition).
fn assignment_first_child<'pr>(node: &Node<'pr>) -> Option<(usize, usize)> {
    if let Some(c) = node.as_call_node() {
        return c.receiver().map(|r| range(&r));
    }
    macro_rules! value {
        ($($as:ident),*) => {
            $(if let Some(n) = node.$as() { return Some(range(&n.value())); })*
        };
    }
    value!(
        as_local_variable_write_node,
        as_instance_variable_write_node,
        as_class_variable_write_node,
        as_global_variable_write_node,
        as_constant_write_node
    );
    if let Some(n) = node.as_constant_path_write_node() {
        let t = n.target();
        return Some(match t.parent() {
            Some(p) => range(&p),
            None => (t.location().start_offset(), t.delimiter_loc().end_offset()),
        });
    }
    if let Some(m) = node.as_multi_write_node() {
        let lefts: Vec<Node> = m.lefts().iter().chain(m.rest()).chain(m.rights().iter()).collect();
        let start = m.lparen_loc().map_or_else(|| lefts.first().map_or(0, |n| n.location().start_offset()), |l| l.start_offset());
        let end = m.rparen_loc().map_or_else(|| lefts.last().map_or(0, |n| n.location().end_offset()), |r| r.end_offset());
        return Some((start, end));
    }
    // op_asgn / or_asgn / and_asgn: the target, up to the operator.
    let (op, _) = assignment_parts(node)?;
    Some((node.location().start_offset(), trim_end(node, op.start_offset())))
}

/// End of the target text before an operator (trailing spaces dropped).
fn trim_end(node: &Node, op_start: usize) -> usize {
    let start = node.location().start_offset();
    let text = &node_text(node)[..op_start - start];
    start + text.trim_end().len()
}

/// `node_parts[0].to_s` of an assignment, when it can equal a source string.
fn assigned_name(node: &Node) -> Option<String> {
    if let Some(c) = node.as_call_node() {
        return if is_csend(&c) { None } else { c.receiver().map(|r| node_text(&r).to_string()) };
    }
    let name = match node {
        Node::LocalVariableWriteNode { .. } => node.as_local_variable_write_node().unwrap().name(),
        Node::InstanceVariableWriteNode { .. } => node.as_instance_variable_write_node().unwrap().name(),
        Node::ClassVariableWriteNode { .. } => node.as_class_variable_write_node().unwrap().name(),
        Node::GlobalVariableWriteNode { .. } => node.as_global_variable_write_node().unwrap().name(),
        Node::ConstantWriteNode { .. } => return Some(String::new()),
        _ => return None,
    };
    Some(String::from_utf8_lossy(name.as_slice()).into_owned())
}

/// Visitor: does any parser node start at or after `from` and end by `to`?
struct After {
    from: usize,
    to: usize,
    found: bool,
}

impl After {
    fn check(&mut self, node: &Node) {
        let (s, e) = range(node);
        if s >= self.from && e <= self.to && !matches!(node, Node::StatementsNode { .. } | Node::ElseNode { .. }) {
            self.found = true;
        }
    }
}

impl<'pr> ruby_prism::Visit<'pr> for After {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
}

impl Cop for IdenticalConditionalBranches {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::CASE_NODE, kind::CASE_MATCH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let mut branches = Vec::new();
        let mut keep_correction = true;
        let condition: Option<Node> = match node {
            Node::IfNode { .. } => {
                let i = node.as_if_node().unwrap();
                if i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif") || is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                // ternary / `then`: the corrector does nothing
                keep_correction =
                    i.if_keyword_loc().is_some() && !i.then_keyword_loc().is_some_and(|t| t.as_slice() == b"then");
                branches.push(Branch::of(i.statements()));
                expand_elses(else_of_if(&i), &mut branches);
                Some(i.predicate())
            }
            Node::UnlessNode { .. } => {
                let u = node.as_unless_node().unwrap();
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                keep_correction = !u.then_keyword_loc().is_some_and(|t| t.as_slice() == b"then");
                branches.push(Branch::of(u.statements()));
                expand_elses(Branch::of(u.else_clause().and_then(|e| e.statements())), &mut branches);
                Some(u.predicate())
            }
            Node::CaseNode { .. } => {
                let c = node.as_case_node().unwrap();
                let Some(e) = c.else_clause().and_then(|e| e.statements()) else { return };
                for w in c.conditions().iter() {
                    branches.push(Branch::of(w.as_when_node().unwrap().statements()));
                }
                branches.push(Branch::of(Some(e)));
                c.predicate()
            }
            _ => {
                let c = node.as_case_match_node().unwrap();
                let Some(e) = c.else_clause().and_then(|e| e.statements()) else { return };
                for w in c.conditions().iter() {
                    branches.push(Branch::of(w.as_in_node().unwrap().statements()));
                }
                branches.push(Branch::of(Some(e)));
                c.predicate()
            }
        };
        if branches.iter().any(|b| matches!(b, Branch::Nil)) {
            return;
        }
        let src = cx.src;
        let node_range = range(node);

        // Parser parent: the nearest parser-shaped ancestor.
        let parents = parser_ancestor_ranges(cx.ancestors);
        let parent = parents.first().map(|&(i, s, e)| {
            let p = &cx.ancestors[i];
            let e = p.as_call_node().map_or(e, |c| send_range(&c).1);
            (p, s, e)
        });

        let duplicated = |exprs: &[Option<&Node>]| -> Option<bool> {
            let Some(first) = exprs[0] else { return Some(exprs.iter().all(|e| e.is_none())) };
            let fr = range(first);
            if !exprs.iter().all(|e| {
                e.is_some_and(|e| helpers::node_equality::same(src, fr, range(e)))
            }) {
                return Some(false);
            }
            if !is_assignment_node(first) {
                return Some(true);
            }
            let Some(lhs) = assignment_first_child(first) else { return Some(true) };
            let lhs_src = &src.text[lhs.0..lhs.1];
            // `node.condition` is nil for a `case` without subject: RuboCop crashes.
            let cond = condition.as_ref()?;
            Some(!condition_children(cond).iter().any(|n| is_variable(n) && node_text(n) == lhs_src))
        };

        let assignment_parent = parent.as_ref().is_some_and(|(p, _, _)| is_assignment_node(p));
        let check_expressions = |cx: &mut Cx, exprs: &[Option<&Node>], after: bool| {
            if exprs.iter().any(|e| e.is_none()) {
                return;
            }
            // `inserted_expression` is local to each check_expressions call.
            let inserted = Cell::new(false);
            for e in exprs.iter().flatten() {
                let (s, en) = range(e);
                let expr_src = &src.text[s..en];
                let msg = format!("Move `{expr_src}` out of the conditional.");
                let lines = helpers::whole_lines::range_by_whole_lines(src.text, s, en, true);
                let inserted = &inserted;
                let parent = parent.as_ref();
                cx.add_fix(s, en, msg, |c| {
                    if !keep_correction {
                        return;
                    }
                    c.remove(lines.0, lines.1);
                    if inserted.get() {
                        return;
                    }
                    if assignment_parent {
                        let (_, ps, pe) = *parent.unwrap();
                        let indent = " ".repeat(src.column(ps));
                        if after {
                            let assignment = &src.text[ps..node_range.0];
                            c.remove(ps, node_range.0);
                            c.insert_after(node_range.0, node_range.1, &format!("\n{indent}{assignment}{expr_src}"));
                        } else {
                            c.insert_before(ps, pe, &format!("{expr_src}\n{indent}"));
                        }
                    } else {
                        let indent = " ".repeat(src.column(node_range.0));
                        if after {
                            c.insert_after(node_range.0, node_range.1, &format!("\n{indent}{expr_src}"));
                        } else {
                            c.insert_before(node_range.0, node_range.1, &format!("{expr_src}\n{indent}"));
                        }
                    }
                    inserted.set(true);
                });
            }
        };

        let tails: Vec<Option<&Node>> = branches.iter().map(|b| b.tail()).collect();
        match duplicated(&tails) {
            None => return,
            Some(true) => check_expressions(cx, &tails, true),
            Some(false) => {}
        }

        // last_child_of_parent?: no parser sibling after the node inside its parent.
        let last_child = match &parent {
            None => true,
            Some((p, _, pe)) => {
                let mut v = After { from: node_range.1, to: *pe, found: false };
                ruby_prism::Visit::visit(&mut v, p);
                !v.found
            }
        };
        if last_child && branches.iter().any(|b| b.single_child()) {
            return;
        }

        let heads: Vec<Option<&Node>> = branches.iter().map(|b| b.head()).collect();
        if duplicated(&heads) != Some(true) {
            return;
        }
        // assignable_condition_value runs first: a `case` without subject crashes RuboCop here.
        let Some(cond) = condition.as_ref() else { return };
        if let Some(head) = heads[0].filter(|h| is_assignment_node(h)) {
            let cond_value = if let Some(c) = cond.as_call_node() {
                Some(c.receiver().map_or_else(|| node_text(cond), |r| node_text(&r)).to_string())
            } else if is_variable(cond) {
                Some(node_text(cond).to_string())
            } else {
                None
            };
            if cond_value.is_some() && cond_value == assigned_name(head) {
                return;
            }
        }
        check_expressions(cx, &heads, false);
    }
}

fn is_assignment_node(node: &Node) -> bool {
    helpers::node_types::is_assignment(node)
}
