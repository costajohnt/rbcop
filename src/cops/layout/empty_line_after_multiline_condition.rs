use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLineAfterMultilineCondition";
pub static COP: EmptyLineAfterMultilineCondition = EmptyLineAfterMultilineCondition;
pub struct EmptyLineAfterMultilineCondition;

const MSG: &str = "Use empty line after multiline condition.";

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// `processed_source[line].blank?` (`line` is 1-based, so this is the following line).
fn next_line_empty(cx: &Cx, line: usize) -> bool {
    cx.src.lines.get(line).is_none_or(|l| helpers::ruby_string::blank(l))
}

/// `add_offense(range) { insert_after(range_by_whole_lines(node), "\n") }`.
fn report(cx: &mut Cx, offense: (usize, usize), node: (usize, usize)) {
    let text = cx.src.text;
    let (s, e) = helpers::whole_lines::range_by_whole_lines(text, node.0, node.1, false);
    cx.add_fix(offense.0, offense.1, MSG, |c| c.insert_after(s, e, "\n"));
}

fn check_condition(cx: &mut Cx, cond: &Node) {
    let (s, e) = range(cond);
    let last = cx.src.line_of(e);
    // rubocop-ast BlockNode#multiline? compares the `{`/`do` line with the `}`/`end` line.
    let block = match cond {
        Node::CallNode { .. } => cond.as_call_node().unwrap().block().and_then(|b| b.as_block_node()).map(|b| (b.opening_loc(), b.closing_loc())),
        Node::LambdaNode { .. } => cond.as_lambda_node().map(|l| (l.opening_loc(), l.closing_loc())),
        _ => None,
    };
    let first = block.as_ref().map_or(cx.src.line_of(s), |(o, _)| cx.src.line_of(o.start_offset()));
    let close = block.as_ref().map_or(last, |(_, c)| cx.src.line_of(c.start_offset()));
    if first == close || next_line_empty(cx, last) {
        return;
    }
    report(cx, (s, e), (s, e));
}

/// rubocop-ast `right_sibling` is non-nil for a statement (`ancestors.last()` its Prism parent).
fn has_right_sibling(node: &Node, anc: &[Node]) -> bool {
    let Some(stmts) = anc.last().and_then(|p| p.as_statements_node()) else { return false };
    let body: Vec<Node> = stmts.body().iter().collect();
    let start = node.location().start_offset();
    if body.last().is_some_and(|l| l.location().start_offset() != start) {
        return true;
    }
    if body.len() > 1 {
        return false;
    }
    let is_stmts = |s: Option<ruby_prism::StatementsNode>| {
        s.is_some_and(|s| s.location().start_offset() == stmts.location().start_offset())
    };
    let nonempty_else = |e: Option<ruby_prism::ElseNode>| e.is_some_and(|e| e.statements().is_some());
    match anc.len().checked_sub(2).map(|i| &anc[i]) {
        Some(Node::IfNode { .. }) => {
            let i = anc[anc.len() - 2].as_if_node().unwrap();
            is_stmts(i.statements())
                && i.subsequent().is_some_and(|s| s.as_else_node().is_none() || nonempty_else(s.as_else_node()))
        }
        Some(Node::ElseNode { .. }) => {
            // The else branch of an `unless` is parser's if_branch, followed by the unless body.
            let i = anc.len() - 3;
            anc.get(i).and_then(|u| u.as_unless_node()).is_some_and(|u| u.statements().is_some())
        }
        Some(Node::BeginNode { .. }) => {
            let b = anc[anc.len() - 2].as_begin_node().unwrap();
            is_stmts(b.statements())
                && (b.rescue_clause().is_some()
                    || b.ensure_clause().is_some_and(|e| e.statements().is_some()))
        }
        _ => false,
    }
}

impl Cop for EmptyLineAfterMultilineCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::WHILE_NODE, kind::UNTIL_NODE, kind::CASE_NODE, kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node {
            Node::IfNode { .. } | Node::UnlessNode { .. } => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                let c = helpers::if_node::cond(node).unwrap();
                if c.ternary() || (c.modifier() && !has_right_sibling(node, cx.ancestors)) {
                    return;
                }
                check_condition(cx, &c.condition);
            }
            Node::WhileNode { .. } | Node::UntilNode { .. } => {
                let (cond, post) = match node.as_while_node() {
                    Some(w) => (w.predicate(), w.is_begin_modifier()),
                    None => {
                        let u = node.as_until_node().unwrap();
                        (u.predicate(), u.is_begin_modifier())
                    }
                };
                if post && !has_right_sibling(node, cx.ancestors) {
                    return;
                }
                check_condition(cx, &cond);
            }
            Node::CaseNode { .. } => {
                for w in node.as_case_node().unwrap().conditions().iter() {
                    let w = w.as_when_node().unwrap();
                    let conds: Vec<Node> = w.conditions().iter().collect();
                    let (Some(first), Some(last)) = (conds.first(), conds.last()) else { continue };
                    let last_line = cx.src.line_of(range(last).1);
                    if cx.src.line_of(range(first).0) == last_line || next_line_empty(cx, last_line) {
                        continue;
                    }
                    // Parser's `when` ends at its body, or at the last condition without one.
                    let end = w.statements().map_or(range(last).1, |s| s.location().end_offset());
                    report(cx, (w.keyword_loc().start_offset(), end), range(last));
                }
            }
            _ => {
                let r = node.as_rescue_node().unwrap();
                let exc: Vec<Node> = r.exceptions().iter().collect();
                if exc.len() <= 1 {
                    return;
                }
                let last = range(exc.last().unwrap());
                let last_line = cx.src.line_of(last.1);
                if cx.src.line_of(range(&exc[0]).0) == last_line || next_line_empty(cx, last_line) {
                    return;
                }
                let resbody = helpers::resbody::range(cx.src, &r);
                report(cx, resbody, last);
            }
        }
    }
}
