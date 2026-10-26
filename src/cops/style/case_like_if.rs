use crate::prelude::*;
use crate::source::Source;
use helpers::literal_node::literal;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/CaseLikeIf";
pub static COP: CaseLikeIf = CaseLikeIf;
pub struct CaseLikeIf;

type R = (usize, usize);

fn rng(n: &Node) -> R {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// rubocop-ast `else_branch` of an if / unless / ternary (a single `else` statement is the node itself).
fn else_branch<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let stmts = if let Some(i) = node.as_if_node() {
        let s = i.subsequent()?;
        if s.as_if_node().is_some() {
            return Some(s);
        }
        s.as_else_node()?.statements()?
    } else {
        node.as_unless_node()?.else_clause()?.statements()?
    };
    let body: Vec<Node> = stmts.body().iter().collect();
    match body.len() {
        1 => body.into_iter().next(),
        _ => Some(stmts.as_node()),
    }
}

fn predicate_and_keyword<'pr>(n: &Node<'pr>) -> Option<(Node<'pr>, usize)> {
    if let Some(i) = n.as_if_node() {
        return Some((i.predicate(), i.if_keyword_loc()?.start_offset()));
    }
    let u = n.as_unless_node()?;
    Some((u.predicate(), u.keyword_loc().start_offset()))
}

/// `branch_conditions`: the condition and keyword offset of the `if` and each `elsif`.
fn branch_conditions<'pr>(node: &Node<'pr>) -> Vec<(Node<'pr>, usize)> {
    let mut out: Vec<_> = predicate_and_keyword(node).into_iter().collect();
    let mut next = else_branch(node);
    while let Some(e) = next {
        let Some(i) = e.as_if_node().filter(|i| i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif")) else {
            break;
        };
        out.push((i.predicate(), i.if_keyword_loc().unwrap().start_offset()));
        next = else_branch(&e);
    }
    out
}

/// `if_conditional_branches(node).size` (MinBranchesCount): the `if` and its `elsif`s.
fn branch_count(node: &Node) -> usize {
    branch_conditions(node).len()
}

/// Parenthesised first statement (`node.children.first` of a `begin`).
fn first_in_parens<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    let body = n.as_parentheses_node()?.body()?;
    match body.as_statements_node() {
        Some(s) => s.body().iter().next(),
        None => Some(body),
    }
}

/// A `send` (or parser `match_with_lvasgn`, which behaves the same here).
fn as_send<'pr>(n: &Node<'pr>) -> Option<CallNode<'pr>> {
    if let Some(m) = n.as_match_write_node() {
        return Some(m.call());
    }
    n.as_call_node().filter(|c| !is_csend(c))
}

fn first_arg<'pr>(c: &CallNode<'pr>) -> Option<Node<'pr>> {
    call_args(c).into_iter().next()
}

fn is_regexp(n: &Node) -> bool {
    matches!(kind::of(n), kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE)
}

fn is_range(n: &Option<Node>) -> bool {
    n.as_ref().is_some_and(|n| n.as_range_node().is_some())
}

fn const_name<'pr>(n: &Node<'pr>) -> Option<&'pr [u8]> {
    if let Some(c) = n.as_constant_read_node() {
        return Some(c.name().as_slice());
    }
    Some(n.as_constant_path_node()?.name()?.as_slice())
}

fn const_reference(n: &Node) -> bool {
    const_name(n).is_some_and(|b| {
        let s = String::from_utf8_lossy(b);
        s.chars().count() > 1 && s.to_uppercase() == s
    })
}

fn class_reference(n: &Node) -> bool {
    const_name(n).is_some_and(|b| String::from_utf8_lossy(b).chars().any(char::is_lowercase))
}

fn find_target<'pr>(n: Node<'pr>) -> Option<Node<'pr>> {
    if n.as_parentheses_node().is_some() {
        return find_target(first_in_parens(&n)?);
    }
    if let Some(o) = n.as_or_node() {
        return find_target(o.left());
    }
    let call = as_send(&n)?;
    match method_name(&call) {
        "is_a?" => call.receiver(),
        "==" | "eql?" | "equal?" => {
            let (arg, recv) = (first_arg(&call)?, call.receiver()?);
            if literal(&arg) || const_reference(&arg) {
                Some(recv)
            } else if literal(&recv) || const_reference(&recv) {
                Some(arg)
            } else {
                None
            }
        }
        "===" => first_arg(&call),
        "include?" | "cover?" => {
            let recv = call.receiver()?;
            if is_range(&strip_parens(recv)) { first_arg(&call) } else { None }
        }
        "match" | "match?" | "=~" => {
            let recv = call.receiver()?;
            if is_regexp(&recv) {
                first_arg(&call)
            } else if is_regexp(&first_arg(&call)?) {
                Some(recv)
            } else {
                None
            }
        }
        _ => None,
    }
}

struct Collector<'s, 'a> {
    src: &'s Source<'a>,
    target: R,
}

impl Collector<'_, '_> {
    fn eq(&self, n: &Node) -> bool {
        helpers::node_equality::same(self.src, rng(n), self.target)
    }

    fn binary_op(&self, lhs: Option<Node>, rhs: Option<Node>) -> Option<R> {
        let (lhs, rhs) = (strip_parens(lhs?)?, strip_parens(rhs?)?);
        if self.eq(&lhs) {
            Some(rng(&rhs))
        } else if self.eq(&rhs) {
            Some(rng(&lhs))
        } else {
            None
        }
    }

    fn condition_from_send(&self, call: &CallNode) -> Option<R> {
        match method_name(call) {
            "is_a?" => {
                if call.receiver().is_some_and(|r| self.eq(&r)) { first_arg(call).map(|a| rng(&a)) } else { None }
            }
            "==" | "eql?" | "equal?" => {
                let (lhs, rhs) = (strip_parens(call.receiver()?)?, strip_parens(first_arg(call)?)?);
                let c = if self.eq(&lhs) {
                    rhs
                } else if self.eq(&rhs) {
                    lhs
                } else {
                    return None;
                };
                (!class_reference(&c)).then(|| rng(&c))
            }
            "=~" | "match" | "match?" => self.binary_op(call.receiver(), first_arg(call)),
            "===" => {
                if first_arg(call).is_some_and(|a| self.eq(&a)) { call.receiver().map(|r| rng(&r)) } else { None }
            }
            "include?" | "cover?" => {
                let recv = strip_parens(call.receiver()?)?;
                (recv.as_range_node().is_some() && first_arg(call).is_some_and(|a| self.eq(&a))).then(|| rng(&recv))
            }
            _ => None,
        }
    }

    fn collect(&self, n: &Node, out: &mut Vec<R>) -> bool {
        if n.as_parentheses_node().is_some() {
            return first_in_parens(n).is_some_and(|f| self.collect(&f, out));
        }
        if let Some(o) = n.as_or_node() {
            return self.collect(&o.left(), out) && self.collect(&o.right(), out);
        }
        match as_send(n).and_then(|c| self.condition_from_send(&c)) {
            Some(r) => {
                out.push(r);
                true
            }
            None => false,
        }
    }
}

fn named_captures(n: &Node) -> bool {
    is_regexp(n) && helpers::regexp_captures::regexp_captures(n).is_some_and(|(named, _)| named > 0)
}

/// `regexp_with_working_captures?`
fn working_captures(n: &Node) -> bool {
    if let Some(m) = n.as_match_write_node() {
        return m.call().receiver().is_some_and(|r| named_captures(&r));
    }
    let Some(call) = n.as_call_node().filter(|c| !is_csend(c)) else { return false };
    match method_name(&call) {
        // a static regexp on the left makes parser's match_with_lvasgn
        "=~" => call.receiver().is_some_and(|r| r.as_regular_expression_node().is_some() && named_captures(&r)),
        "match" => call.receiver().is_some_and(|r| named_captures(&r)) || first_arg(&call).is_some_and(|a| named_captures(&a)),
        _ => false,
    }
}

impl Cop for CaseLikeIf {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let n = node.as_if_node().unwrap();
        if n.if_keyword_loc().is_none_or(|k| k.as_slice() != b"if") || n.end_keyword_loc().is_none() {
            return;
        }
        if !n.subsequent().is_some_and(|s| s.as_if_node().is_some()) || is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let min = cx.cfg.positive_int("MinBranchesCount", 3);
        if branch_count(node) < min {
            return;
        }
        let Some(target) = find_target(n.predicate()) else { return };
        let col = Collector { src: cx.src, target: rng(&target) };
        let mut edits = Vec::new();
        for (cond, kw) in branch_conditions(node) {
            if working_captures(&cond) {
                return;
            }
            let mut conds = Vec::new();
            if !col.collect(&cond, &mut conds) {
                return;
            }
            let text: Vec<&str> = conds.iter().map(|&(s, e)| &cx.src.text[s..e]).collect();
            edits.push((kw, rng(&cond).1, format!("when {}", text.join(", "))));
        }
        let (start, end) = rng(node);
        let head = format!("case {}\n{}", node_text(&target), " ".repeat(cx.src.column(start)));
        cx.add_fix(start, end, "Convert `if-elsif` to `case-when`.", |c| {
            c.insert_before(start, end, &head);
            for (s, e, t) in &edits {
                c.replace(*s, *e, t);
            }
        });
    }
}
