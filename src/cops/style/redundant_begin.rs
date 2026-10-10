use crate::helpers::node_types::is_assignment;
use crate::helpers::parser_parent::{is_operator_keyword, parser_parent};
use crate::helpers::range_help::{Surrounding, range_with_surrounding_space};
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;
use ruby_prism::BeginNode;

pub const NAME: &str = "Style/RedundantBegin";
pub static COP: RedundantBegin = RedundantBegin;
pub struct RedundantBegin;

const MSG: &str = "Redundant `begin` block detected.";

enum Op {
    Replace(usize, usize, String),
    Remove(usize, usize),
    InsertBefore(usize, usize, String),
    InsertAfter(usize, usize, String),
}

/// A kwbegin seen in pre-order: its range and, when not `allowable_kwbegin?`, its offense.
struct Kwbegin {
    start: usize,
    end: usize,
    offense: Option<(usize, usize, Vec<Op>)>,
}

#[derive(Default)]
struct State(Vec<Kwbegin>);

impl Cop for RedundantBegin {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::DEF_NODE,
            kind::IF_NODE,
            kind::UNLESS_NODE,
            kind::CASE_NODE,
            kind::CASE_MATCH_NODE,
            kind::WHILE_NODE,
            kind::UNTIL_NODE,
            kind::BLOCK_NODE,
            kind::BEGIN_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match kind::of(node) {
            kind::DEF_NODE => {
                let def = node.as_def_node().unwrap();
                if def.equal_loc().is_some() {
                    return;
                }
                if let Some(b) = kwbegin_body(def.body()) {
                    register_simple(cx, &b);
                }
            }
            kind::IF_NODE | kind::UNLESS_NODE => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                for branch in if_branches(node) {
                    inspect_branch(cx, branch);
                }
            }
            kind::CASE_NODE => {
                let n = node.as_case_node().unwrap();
                for w in n.conditions().iter() {
                    inspect_branch(cx, w.as_when_node().and_then(|w| w.statements()).map(|s| s.as_node()));
                }
                inspect_branch(cx, n.else_clause().and_then(|e| e.statements()).map(|s| s.as_node()));
            }
            kind::CASE_MATCH_NODE => {
                let n = node.as_case_match_node().unwrap();
                for w in n.conditions().iter() {
                    inspect_branch(cx, w.as_in_node().and_then(|w| w.statements()).map(|s| s.as_node()));
                }
                inspect_branch(cx, n.else_clause().and_then(|e| e.statements()).map(|s| s.as_node()));
            }
            kind::WHILE_NODE | kind::UNTIL_NODE => {
                let (closing, post, body) = match node.as_while_node() {
                    Some(w) => (w.closing_loc(), w.is_begin_modifier(), w.statements()),
                    None => {
                        let u = node.as_until_node().unwrap();
                        (u.closing_loc(), u.is_begin_modifier(), u.statements())
                    }
                };
                if closing.is_none() || post {
                    return;
                }
                inspect_branch(cx, body.map(|s| s.as_node()));
            }
            kind::BLOCK_NODE => {
                let b = node.as_block_node().unwrap();
                if cx.config.target_ruby < 2.5 || b.opening_loc().as_slice() == b"{" {
                    return;
                }
                if let Some(k) = kwbegin_body(b.body()) {
                    register_simple(cx, &k);
                }
            }
            _ => {
                let Some(b) = node.as_begin_node().filter(|b| b.begin_keyword_loc().is_some()) else { return };
                let parent = parser_parent(cx.ancestors);
                let loc = node.location();
                let offense = (!allowable(&b, parent)).then(|| {
                    let kw = b.begin_keyword_loc().unwrap();
                    (kw.start_offset(), kw.end_offset(), plan(cx.src, &b, parent))
                });
                cx.state::<State>().0.push(Kwbegin { start: loc.start_offset(), end: loc.end_offset(), offense });
            }
        }
    }

    fn on_file(&self, cx: &mut Cx) {
        // on_kwbegin: register the last offensive kwbegin (pre-order) in each kwbegin's subtree.
        let Some(state) = cx.state.take() else { return };
        let Ok(state) = state.downcast::<State>() else { return };
        let all = &state.0;
        for k in all {
            let last = all.iter().filter(|o| o.offense.is_some() && k.start <= o.start && o.end <= k.end).max_by_key(|o| o.start);
            if let Some((s, e, ops)) = last.and_then(|o| o.offense.as_ref()) {
                cx.add_fix(*s, *e, MSG, |c| apply(c, ops));
            }
        }
    }
}

fn apply(c: &mut Corrector, ops: &[Op]) {
    for op in ops {
        match op {
            Op::Replace(s, e, t) => c.replace(*s, *e, t),
            Op::Remove(s, e) => c.remove(*s, *e),
            Op::InsertBefore(s, e, t) => c.insert_before(*s, *e, t),
            Op::InsertAfter(s, e, t) => c.insert_after(*s, *e, t),
        }
    }
}

/// A body that is a single kwbegin (`kwbegin_type?`).
fn kwbegin_body(body: Option<Node<'_>>) -> Option<BeginNode<'_>> {
    let stmts: Vec<_> = body?.as_statements_node()?.body().iter().collect();
    match stmts.as_slice() {
        [only] => only.as_begin_node().filter(|b| b.begin_keyword_loc().is_some()),
        _ => None,
    }
}

/// inspect_branches: a kwbegin branch without rescue/ensure.
fn inspect_branch(cx: &mut Cx, branch: Option<Node>) {
    let Some(b) = kwbegin_body(branch) else { return };
    if b.rescue_clause().is_none() && b.ensure_clause().is_none() {
        register_simple(cx, &b);
    }
}

/// register_offense where the parent is a def/block/loop/branching node: remove `begin` and `end`.
fn register_simple(cx: &mut Cx, b: &BeginNode) {
    let kw = b.begin_keyword_loc().unwrap();
    let end = b.end_keyword_loc();
    cx.add_fix(kw.start_offset(), kw.end_offset(), MSG, |c| {
        c.remove(kw.start_offset(), kw.end_offset());
        if let Some(end) = end {
            c.remove(end.start_offset(), end.end_offset());
        }
    });
}

/// rubocop-ast IfNode#branches, as Prism statement holders.
fn if_branches<'pr>(node: &Node<'pr>) -> Vec<Option<Node<'pr>>> {
    if let Some(u) = node.as_unless_node() {
        if u.end_keyword_loc().is_none() {
            return vec![];
        }
        let mut out = vec![u.statements().map(|s| s.as_node())];
        if let Some(e) = u.else_clause() {
            out.push(e.statements().map(|s| s.as_node()));
        }
        return out;
    }
    let n = node.as_if_node().unwrap();
    let ternary = n.if_keyword_loc().is_none();
    if !ternary && n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && n.end_keyword_loc().is_none() {
        return vec![];
    }
    let mut out = vec![n.statements().map(|s| s.as_node())];
    match n.subsequent() {
        Some(s) if ternary => out.push(s.as_else_node().unwrap().statements().map(|s| s.as_node())),
        Some(s) if s.as_if_node().is_some() => out.extend(if_branches(&s)),
        Some(s) => out.push(s.as_else_node().unwrap().statements().map(|s| s.as_node())),
        None => {}
    }
    out
}

fn children<'pr>(b: &BeginNode<'pr>) -> Vec<Node<'pr>> {
    b.statements().map(|s| s.body().iter().collect()).unwrap_or_default()
}

/// allowable_kwbegin?
fn allowable(b: &BeginNode, parent: Option<&Node>) -> bool {
    let n = children(b).len();
    if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
        return true;
    }
    if n == 0 || (parent.is_some() && n >= 2) {
        return true;
    }
    let Some(p) = parent else { return false };
    (is_assignment(p) && n != 1)
        || p.as_while_node().is_some_and(|w| w.is_begin_modifier())
        || p.as_until_node().is_some_and(|w| w.is_begin_modifier())
        || p.as_call_node().is_some_and(|c| !is_csend(&c))
        || is_operator_keyword(p)
}

/// The corrector block of register_offense for a kwbegin with its parser parent.
fn plan(src: &crate::source::Source, b: &BeginNode, parent: Option<&Node>) -> Vec<Op> {
    let kw = b.begin_keyword_loc().unwrap();
    let (ks, ke) = (kw.start_offset(), kw.end_offset());
    let first = children(b).into_iter().next();
    let mut ops = Vec::new();
    match (parent, &first) {
        (Some(p), Some(first)) if is_assignment(p) => {
            let mut source = node_text(first).to_string();
            if modifier_if(first).is_some() {
                source = format!("({source})");
            }
            let (fs, fe) = (first.location().start_offset(), first.location().end_offset());
            ops.push(Op::Replace(ks, ke, source));
            ops.push(Op::Remove(ke, fe));
            let comments = &src.text[ke..fs];
            if !comments.chars().all(|c| c.is_whitespace() || c == '\0') {
                let loc = p.location();
                ops.push(Op::InsertBefore(loc.start_offset(), loc.end_offset(), comments.to_string()));
            }
        }
        _ => {
            let endless = parent.and_then(|p| p.as_def_node()).is_some_and(|d| d.equal_loc().is_some());
            let (s, e) = if endless { range_with_surrounding_space(src.text, ks, ke, Surrounding::default()) } else { (ks, ke) };
            ops.push(Op::Remove(s, e));
        }
    }
    let loc = b.as_node().location();
    let multiline = src.line_of(loc.start_offset()) != src.line_of(loc.end_offset());
    if let (true, Some((cs, ce)), Some(first)) = (multiline, parent.and_then(modifier_if), &first) {
        let (fs, fe) = (first.location().start_offset(), first.location().end_offset());
        ops.push(Op::InsertAfter(fs, fe, format!(" {}", &src.text[cs..ce])));
        let (ls, le) = range_by_whole_lines(src.text, cs, ce, true);
        ops.push(Op::Remove(ls, le));
    }
    if let Some(end) = b.end_keyword_loc() {
        ops.push(Op::Remove(end.start_offset(), end.end_offset()));
    }
    ops
}

/// A modifier if/unless: the range from its keyword to the end of its condition.
fn modifier_if(node: &Node) -> Option<(usize, usize)> {
    if let Some(n) = node.as_if_node() {
        let kw = n.if_keyword_loc().filter(|k| k.as_slice() == b"if" && n.end_keyword_loc().is_none())?;
        return Some((kw.start_offset(), n.predicate().location().end_offset()));
    }
    let n = node.as_unless_node().filter(|u| u.end_keyword_loc().is_none())?;
    Some((n.keyword_loc().start_offset(), n.predicate().location().end_offset()))
}
