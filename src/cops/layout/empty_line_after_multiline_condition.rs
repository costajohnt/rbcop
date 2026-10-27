use crate::prelude::*;
use helpers::rspec::right_sibling;

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

/// body_on_line?: the parser body (a statement list's first statement) starts on `line`.
fn body_on_line(cx: &Cx, body: Option<usize>, line: usize) -> bool {
    body.is_some_and(|start| cx.src.line_of(start) == line)
}

fn stmts_start(s: Option<ruby_prism::StatementsNode>) -> Option<usize> {
    s.map(|s| s.location().start_offset())
}

/// `add_offense(range) { insert_after(range_by_whole_lines(node), "\n") }`.
fn report(cx: &mut Cx, offense: (usize, usize), node: (usize, usize)) {
    let text = cx.src.text;
    let (s, e) = helpers::whole_lines::range_by_whole_lines(text, node.0, node.1, false);
    cx.add_fix(offense.0, offense.1, MSG, |c| c.insert_after(s, e, "\n"));
}

/// check_condition(condition, following_node): `following` is the start of the following node.
fn check_condition(cx: &mut Cx, cond: &Node, following: Option<usize>) {
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
    if first == close || next_line_empty(cx, last) || body_on_line(cx, following, last) {
        return;
    }
    report(cx, (s, e), (s, e));
}

/// followed_by_statement?: the node is in a parser `begin`/`kwbegin` and has a right sibling;
/// the sibling's start.
fn followed_by_statement(cx: &Cx, node: &Node) -> Option<usize> {
    right_sibling(cx.ancestors, node).map(|s| s.location().start_offset())
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
                if c.ternary() {
                    return;
                }
                let following = if c.modifier() {
                    let Some(next) = followed_by_statement(cx, node) else { return };
                    Some(next)
                } else {
                    // IfNode#if_branch: parser's second child, the `else` body of an `unless`.
                    match node.as_if_node() {
                        Some(i) => stmts_start(i.statements()),
                        None => stmts_start(node.as_unless_node().unwrap().else_clause().and_then(|e| e.statements())),
                    }
                };
                check_condition(cx, &c.condition, following);
            }
            Node::WhileNode { .. } | Node::UntilNode { .. } => {
                let (cond, post, modifier, body) = match node.as_while_node() {
                    Some(w) => (w.predicate(), w.is_begin_modifier(), w.closing_loc().is_none(), w.statements()),
                    None => {
                        let u = node.as_until_node().unwrap();
                        (u.predicate(), u.is_begin_modifier(), u.closing_loc().is_none(), u.statements())
                    }
                };
                let following = if post || modifier {
                    let Some(next) = followed_by_statement(cx, node) else { return };
                    Some(next)
                } else {
                    stmts_start(body)
                };
                check_condition(cx, &cond, following);
            }
            Node::CaseNode { .. } => {
                for w in node.as_case_node().unwrap().conditions().iter() {
                    let w = w.as_when_node().unwrap();
                    let conds: Vec<Node> = w.conditions().iter().collect();
                    let (Some(first), Some(last)) = (conds.first(), conds.last()) else { continue };
                    let last_line = cx.src.line_of(range(last).1);
                    if cx.src.line_of(range(first).0) == last_line
                        || next_line_empty(cx, last_line)
                        || body_on_line(cx, stmts_start(w.statements()), last_line)
                    {
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
                if cx.src.line_of(range(&exc[0]).0) == last_line
                    || next_line_empty(cx, last_line)
                    || body_on_line(cx, stmts_start(r.statements()), last_line)
                {
                    return;
                }
                let resbody = helpers::resbody::range(cx.src, &r);
                report(cx, resbody, last);
            }
        }
    }
}
