use crate::prelude::*;
use helpers::range_help::{Side, Surrounding, range_with_surrounding_space};

pub const NAME: &str = "Style/RedundantReturn";
pub static COP: RedundantReturn = RedundantReturn;
pub struct RedundantReturn;

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

impl Cop for RedundantReturn {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(def) = node.as_def_node() {
            return check_body_node(cx, def.body());
        }
        // `-> {}` is `(block (lambda) ...)` in parser; on_send fires for `lambda`.
        if let Some(l) = node.as_lambda_node() {
            return check_body_node(cx, l.body());
        }
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "define_method" | "define_singleton_method" | "lambda") || is_csend(&call) {
            return;
        }
        if let Some(block) = call.block().and_then(|b| b.as_block_node()) {
            check_body_node(cx, block.body());
        }
    }
}

/// A def/block body: a StatementsNode (one statement, or a parser `begin`) or an implicit BeginNode.
fn check_body_node(cx: &mut Cx, body: Option<Node>) {
    match body {
        Some(b) if b.as_statements_node().is_some() => check_statements(cx, b.as_statements_node()),
        Some(b) => check_branch(cx, &b),
        None => {}
    }
}

/// A statement list: its last statement (a single one, or the last child of a parser `begin`).
fn check_statements(cx: &mut Cx, stmts: Option<ruby_prism::StatementsNode>) {
    if let Some(last) = stmts.and_then(|s| s.body().iter().last()) {
        check_branch(cx, &last);
    }
}

fn check_branch(cx: &mut Cx, node: &Node) {
    match kind::of(node) {
        kind::RETURN_NODE => check_return(cx, node),
        kind::CASE_NODE => {
            let c = node.as_case_node().unwrap();
            for w in c.conditions().iter() {
                check_statements(cx, w.as_when_node().unwrap().statements());
            }
            check_statements(cx, c.else_clause().and_then(|e| e.statements()));
        }
        kind::CASE_MATCH_NODE => {
            let c = node.as_case_match_node().unwrap();
            for i in c.conditions().iter() {
                check_statements(cx, i.as_in_node().unwrap().statements());
            }
            check_statements(cx, c.else_clause().and_then(|e| e.statements()));
        }
        kind::IF_NODE => {
            let i = node.as_if_node().unwrap();
            if i.if_keyword_loc().is_none() {
                return; // ternary
            }
            check_statements(cx, i.statements());
            match i.subsequent() {
                Some(s) if s.as_else_node().is_some() => check_statements(cx, s.as_else_node().unwrap().statements()),
                Some(s) => check_branch(cx, &s),
                None => {}
            }
        }
        kind::UNLESS_NODE => {
            let u = node.as_unless_node().unwrap();
            check_statements(cx, u.statements());
            check_statements(cx, u.else_clause().and_then(|e| e.statements()));
        }
        kind::PARENTHESES_NODE => match node.as_parentheses_node().unwrap().body() {
            Some(b) if b.as_statements_node().is_some() => check_statements(cx, b.as_statements_node()),
            Some(b) => check_branch(cx, &b),
            None => {}
        },
        kind::BEGIN_NODE => {
            // kwbegin / ensure / rescue: the rescue bodies and else, or the main body.
            let b = node.as_begin_node().unwrap();
            if let Some(mut r) = b.rescue_clause() {
                loop {
                    check_statements(cx, r.statements());
                    match r.subsequent() {
                        Some(n) => r = n,
                        None => break,
                    }
                }
                match b.else_clause() {
                    Some(e) => check_statements(cx, e.statements()),
                    None => check_statements(cx, b.statements()),
                }
            } else {
                check_statements(cx, b.statements());
            }
        }
        _ => {}
    }
}

fn check_return(cx: &mut Cx, node: &Node) {
    let ret = node.as_return_node().unwrap();
    let mut args: Vec<Node> = ret.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
    // `return()` is parser's `(return (begin))`, whose WrappedArguments are empty.
    if let [only] = args.as_slice()
        && only.as_parentheses_node().is_some_and(|p| p.body().is_none())
    {
        args.clear();
    }
    let allow_multiple = cx.cfg.bool("AllowMultipleReturnValues").unwrap_or(false);
    if allow_multiple && args.len() > 1 {
        return;
    }
    let msg = if !allow_multiple && args.len() > 1 {
        "Redundant `return` detected. To return multiple values, use an array."
    } else {
        "Redundant `return` detected."
    };
    let kw = ret.keyword_loc();
    let (ks, ke) = (kw.start_offset(), kw.end_offset());
    let whole = range(node);
    let first = args.first().map(range);
    let last = args.last().map(range);
    // A braceless hash first argument: its first and last pairs.
    let hash = args.first().and_then(|a| a.as_keyword_hash_node()).map(|h| {
        let els: Vec<Node> = h.elements().iter().collect();
        (range(&els[0]), range(els.last().unwrap()))
    });
    let splat_first = args.iter().any(|a| a.as_splat_node().is_some()).then(|| {
        let f = &args[0];
        (range(f), node_text(f).strip_prefix('*').unwrap_or(node_text(f)).to_string())
    });
    let n = args.len();
    let kw_range = range_with_surrounding_space(cx.src.text, ks, ke, Surrounding { side: Side::Right, ..Default::default() });
    cx.add_fix(ks, ke, msg, |c| {
        if n == 0 {
            c.replace(whole.0, whole.1, "nil");
            return;
        }
        if n > 1 {
            let (f, l) = (first.unwrap(), last.unwrap());
            c.insert_before(f.0, f.1, "[");
            c.insert_after(l.0, l.1, "]");
        } else if let Some((f, l)) = hash {
            c.insert_before(f.0, f.1, "{");
            c.insert_after(l.0, l.1, "}");
        }
        if let Some(((s, e), src)) = &splat_first {
            c.replace(*s, *e, src);
        }
        c.remove(kw_range.0, kw_range.1);
    });
}
