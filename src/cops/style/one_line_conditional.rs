use crate::prelude::*;
use crate::source::Source;
use helpers::node_types::{is_assignment, is_operator_method};
use helpers::parser_parent::{is_operator_keyword, parser_parent};
use ruby_prism::StatementsNode;
use std::cell::Cell;

pub const NAME: &str = "Style/OneLineConditional";
pub static COP: OneLineConditional = OneLineConditional;
pub struct OneLineConditional;

/// A parser branch: None (empty), one statement, or a `begin` of 2+ statements.
#[derive(Clone, Copy)]
enum Branch<'a, 'pr> {
    One(&'a Node<'pr>),
    Many(usize, usize),
}

fn stmts_range(s: &StatementsNode) -> (usize, usize) {
    (s.location().start_offset(), s.location().end_offset())
}

/// Parser node for a branch's statements: the statement itself, or a begin of 2+.
/// A non-empty branch: its statements and their range.
type Br<'pr> = (Vec<Node<'pr>>, (usize, usize));

fn branch<'pr>(s: Option<StatementsNode<'pr>>) -> Option<Br<'pr>> {
    let s = s?;
    let v: Vec<Node> = s.body().iter().collect();
    if v.is_empty() {
        return None;
    }
    Some((v, stmts_range(&s)))
}

/// `begin_type?` of a branch: 2+ statements or a parenthesised expression.
fn is_begin(b: &(Vec<Node>, (usize, usize))) -> bool {
    b.0.len() > 1 || b.0[0].as_parentheses_node().is_some()
}

/// `begin` children count (2+ statements, or the statements inside parentheses).
fn begin_children(b: &(Vec<Node>, (usize, usize))) -> usize {
    if b.0.len() > 1 {
        return b.0.len();
    }
    b.0[0].as_parentheses_node().and_then(|p| p.body()).map_or(0, |body| match body.as_statements_node() {
        Some(s) => s.body().iter().count(),
        None => 1,
    })
}

fn source<'s>(src: &Source<'s>, b: &(Vec<Node>, (usize, usize))) -> &'s str {
    src.slice(b.1 .0, b.1 .1)
}

/// The parts of an if/unless/elsif as rubocop-ast sees them.
struct Parts<'pr> {
    keyword: &'static str,
    cond: Node<'pr>,
    /// `if_branch` (unless: its body).
    if_branch: Option<Br<'pr>>,
    /// `else_branch` (unless: the else clause); an elsif is reported in `elsif`.
    else_branch: Option<Br<'pr>>,
    elsif: Option<Node<'pr>>,
    /// Raw children 1 and 2 (`*node`): unless swaps them.
    raw: [Option<Br<'pr>>; 2],
}

fn parts<'pr>(node: &Node<'pr>) -> Parts<'pr> {
    if let Some(n) = node.as_if_node() {
        let keyword = if n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif") { "elsif" } else { "if" };
        let sub = n.subsequent();
        let elsif = sub.as_ref().and_then(|s| s.as_if_node()).map(|i| i.as_node());
        let else_branch = sub.as_ref().and_then(|s| s.as_else_node()).and_then(|e| branch(e.statements()));
        Parts {
            keyword,
            cond: n.predicate(),
            if_branch: branch(n.statements()),
            else_branch,
            elsif,
            raw: [branch(n.statements()), sub.as_ref().and_then(|s| s.as_else_node()).and_then(|e| branch(e.statements()))],
        }
    } else {
        let n = node.as_unless_node().unwrap();
        let else_branch = n.else_clause().and_then(|e| branch(e.statements()));
        Parts {
            keyword: "unless",
            cond: n.predicate(),
            if_branch: branch(n.statements()),
            else_branch,
            elsif: None,
            raw: [n.else_clause().and_then(|e| branch(e.statements())), branch(n.statements())],
        }
    }
}

impl Cop for OneLineConditional {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        // OnNormalIfUnless: no modifier forms, no ternaries.
        let has_end = match node.as_if_node() {
            // (an elsif is skipped by `node.elsif?`)
            Some(n) => n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && n.end_keyword_loc().is_some(),
            None => node.as_unless_node().unwrap().end_keyword_loc().is_some(),
        };
        if !has_end {
            return;
        }
        let src = cx.src;
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        if src.line_of(start) != src.line_of(end) {
            return;
        }
        let p = parts(node);
        if p.else_branch.is_none() && p.elsif.is_none() {
            return;
        }
        if p.if_branch.as_ref().is_some_and(is_begin) {
            return;
        }
        let multiline = cx.cfg.bool("AlwaysCorrectToMultiline") == Some(true)
            || p.elsif.is_some()
            || p.else_branch.as_ref().is_some_and(|b| is_begin(b) && begin_children(b) >= 2);
        let msg = if multiline {
            format!("Favor multi-line `{}` over single-line `{}/then/else/end` constructs.", p.keyword, p.keyword)
        } else {
            format!("Favor the ternary operator (`?:`) over single-line `{}/then/else/end` constructs.", p.keyword)
        };
        let ignored = cx.state::<Vec<(usize, usize)>>().iter().any(|&(a, b)| a <= start && end <= b);
        let replacement = if multiline {
            let width = helpers::alignment::configured_indentation_width(cx) as usize;
            if_then(src, node, None, &" ".repeat(width))
        } else {
            let r = format!("{} ? {} : {}", expr(src, Some(Branch::One(&p.cond))), raw(src, &p.raw[0]), raw(src, &p.raw[1]));
            let wrap = parser_parent(cx.ancestors).is_some_and(|par| {
                is_operator_keyword(par)
                    || par.as_call_node().is_some_and(|c| {
                        !is_csend(&c)
                            && c.block().is_none_or(|b| b.as_block_node().is_none())
                            && is_operator_method(method_name(&c))
                    })
            });
            if wrap { format!("({r})") } else { r }
        };
        let ran = Cell::new(false);
        cx.add_fix(start, end, msg, |c| {
            if ignored {
                return;
            }
            c.replace(start, end, &replacement);
            ran.set(true);
        });
        if ran.get() {
            cx.state::<Vec<(usize, usize)>>().push((start, end));
        }
    }
}

fn raw(src: &Source, b: &Option<Br>) -> String {
    match b {
        None => "nil".to_string(),
        Some(v) if v.0.len() == 1 => expr(src, Some(Branch::One(&v.0[0]))),
        Some(v) => expr(src, Some(Branch::Many(v.1 .0, v.1 .1))),
    }
}

/// OneLineConditional#expr_replacement.
fn expr(src: &Source, b: Option<Branch>) -> String {
    match b {
        None => "nil".to_string(),
        Some(Branch::Many(s, e)) => src.slice(s, e).to_string(),
        Some(Branch::One(n)) => {
            let text = node_text(n);
            if requires_parentheses(n) { format!("({text})") } else { text.to_string() }
        }
    }
}

fn requires_parentheses(n: &Node) -> bool {
    if matches!(kind::of(n), kind::AND_NODE | kind::OR_NODE | kind::IF_NODE | kind::UNLESS_NODE) || is_assignment(n) {
        return true;
    }
    if let Some(c) = n.as_call_node() {
        let block_literal = c.block().is_some_and(|b| b.as_block_node().is_some());
        let has_args = c.arguments().is_some() || c.block().is_some();
        let parens = c.opening_loc().is_some_and(|o| o.as_slice() == b"(");
        if !is_csend(&c) && !block_literal && has_args && !parens && !is_operator_method(method_name(&c)) {
            return true;
        }
        // keyword_with_changed_precedence?: `not x`
        return !block_literal && method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"not");
    }
    // keyword_with_changed_precedence?: keywords taking unparenthesised arguments.
    match kind::of(n) {
        kind::RETURN_NODE => n.as_return_node().unwrap().arguments().is_some(),
        kind::BREAK_NODE => n.as_break_node().unwrap().arguments().is_some(),
        kind::NEXT_NODE => n.as_next_node().unwrap().arguments().is_some(),
        kind::SUPER_NODE => {
            let s = n.as_super_node().unwrap();
            (s.arguments().is_some() || s.block().is_some_and(|b| b.as_block_argument_node().is_some()))
                && s.lparen_loc().is_none()
        }
        kind::YIELD_NODE => {
            let y = n.as_yield_node().unwrap();
            y.arguments().is_some() && y.lparen_loc().is_none()
        }
        kind::DEFINED_NODE => n.as_defined_node().unwrap().lparen_loc().is_none(),
        _ => false,
    }
}

/// IfThenCorrector#replacement.
fn if_then(src: &Source, node: &Node, indentation: Option<&str>, body_ind: &str) -> String {
    let own = " ".repeat(src.column(node.location().start_offset()));
    let indentation = indentation.unwrap_or(&own);
    let p = parts(node);
    let elsif_ind = if p.keyword == "elsif" { indentation } else { "" };
    let if_src = p.if_branch.as_ref().map_or("nil", |b| source(src, b));
    let mut out = format!(
        "{elsif_ind}{} {}\n{indentation}{body_ind}{if_src}\n",
        p.keyword,
        node_text(&p.cond)
    );
    if let Some(e) = &p.elsif {
        out += &if_then(src, e, Some(indentation), body_ind);
    } else if let Some(b) = &p.else_branch {
        out += &format!("{indentation}else\n{indentation}{body_ind}{}\n{indentation}end", source(src, b));
    } else {
        out += "end";
    }
    out
}
