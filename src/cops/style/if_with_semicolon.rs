use crate::prelude::*;
use crate::source::Source;
use helpers::if_node::{is_elsif, normal_keyword, parts, Body, Else};
use helpers::parser_parent::parser_parent;

pub const NAME: &str = "Style/IfWithSemicolon";
pub static COP: IfWithSemicolon = IfWithSemicolon;
pub struct IfWithSemicolon;

/// `else_branch` as a single parser node that is an `if` (elsif, or a lone if/unless statement).
fn else_if<'pr>(e: &Option<Else<'pr>>) -> Option<Node<'pr>> {
    match e {
        Some(Else::Elsif(n)) => Some(n.as_if_node().unwrap().as_node()),
        Some(Else::Body(Some(b))) => b.single().filter(|n| n.as_if_node().is_some() || n.as_unless_node().is_some()),
        _ => None,
    }
}

fn source<'s>(src: &Source<'s>, b: &Body) -> &'s str {
    let (s, e) = b.range();
    src.slice(s, e)
}

/// IfNode#branches (compacted): a callback per non-empty branch.
fn each_branch(node: &Node, f: &mut dyn FnMut(&Body) -> bool) -> bool {
    let (_, if_branch, els) = parts(node);
    if if_branch.as_ref().is_some_and(&mut *f) {
        return true;
    }
    match &els {
        None => false,
        Some(Else::Elsif(n)) => each_branch(n, f),
        Some(Else::Body(b)) => b.as_ref().is_some_and(f),
    }
}

fn is_block(n: &Node) -> bool {
    let block_literal = |b: Option<Node>| b.is_some_and(|b| b.as_block_node().is_some());
    match kind::of(n) {
        kind::LAMBDA_NODE => true,
        kind::CALL_NODE => block_literal(n.as_call_node().unwrap().block()),
        kind::SUPER_NODE => block_literal(n.as_super_node().unwrap().block()),
        kind::FORWARDING_SUPER_NODE => n.as_forwarding_super_node().unwrap().block().is_some(),
        _ => false,
    }
}

fn require_newline(node: &Node) -> bool {
    each_branch(node, &mut |b| {
        b.is_begin() || b.single().is_some_and(|n| n.as_return_node().is_some_and(|r| r.arguments().is_some()))
    })
}

fn masgn_or_block(node: &Node) -> bool {
    each_branch(node, &mut |b| b.single().is_some_and(|n| n.as_multi_write_node().is_some() || is_block(&n)))
}

/// build_expression; None where RuboCop would raise (no selector).
fn build_expression(src: &Source, b: &Body) -> Option<String> {
    let Some(n) = b.single() else { return Some(source(src, b).to_string()) };
    let text = node_text(&n).to_string();
    let Some(c) = n.as_call_node() else { return Some(text) };
    if c.block().is_some_and(|b| b.as_block_node().is_some()) {
        return Some(text);
    }
    let name = method_name(&c);
    let parens = c.opening_loc().is_some_and(|o| o.as_slice() == b"(");
    let first_arg = c
        .arguments()
        .and_then(|a| a.arguments().iter().next())
        .map(|a| a.location().start_offset())
        .or_else(|| c.block().map(|b| b.location().start_offset()));
    if matches!(name, "+" | "-" | "*" | "/" | "%" | "**") || parens || name == "[]" || name == "[]=" {
        return Some(text);
    }
    let Some(first) = first_arg else { return Some(text) };
    let sel_end = c.message_loc()?.end_offset();
    let (s, e) = (n.location().start_offset(), n.location().end_offset());
    Some(format!("{}({})", src.slice(s, sel_end), src.slice(first, e)))
}

fn opt_source<'s>(src: &Source<'s>, b: &Option<Body>) -> &'s str {
    b.as_ref().map_or("", |b| source(src, b))
}

fn build_else_branch(src: &Source, second: &Node) -> String {
    let (cond, if_branch, els) = parts(second);
    let mut result = format!("elsif {}\n  {}\n", node_text(&cond), opt_source(src, &if_branch));
    if let Some(n) = else_if(&els) {
        result += &build_else_branch(src, &n);
    } else if let Some(Else::Body(b @ Some(_))) = &els {
        result += &format!("else\n  {}\n", opt_source(src, b));
    }
    result
}

fn replacement(src: &Source, node: &Node) -> Option<String> {
    let (cond, if_branch, els) = parts(node);
    if let Some(e) = else_if(&els) {
        let mut tail = build_else_branch(src, &e);
        tail.pop();
        return Some(format!("if {}\n  {}\n{tail}\nend", node_text(&cond), opt_source(src, &if_branch)));
    }
    let then_code = match &if_branch {
        Some(b) => build_expression(src, b)?,
        None => "nil".to_string(),
    };
    let else_code = match &els {
        Some(Else::Body(Some(b))) => build_expression(src, b)?,
        _ => "nil".to_string(),
    };
    let (then_code, else_code) =
        if node.as_unless_node().is_some() { (else_code, then_code) } else { (then_code, else_code) };
    let c = node_text(&cond);
    let c = if assignment_parts(&cond).is_some() { format!("({c})") } else { c.to_string() };
    Some(format!("{c} ? {then_code} : {else_code}"))
}

impl Cop for IfWithSemicolon {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) || normal_keyword(node).is_none() || is_elsif(node) {
            return;
        }
        if parser_parent(cx.ancestors).is_some_and(|p| p.as_if_node().is_some() || p.as_unless_node().is_some()) {
            return;
        }
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        if cx.state::<Vec<(usize, usize)>>().iter().any(|&(a, b)| a <= start && end <= b) {
            return;
        }
        let then = match node.as_if_node() {
            Some(n) => n.then_keyword_loc(),
            None => node.as_unless_node().unwrap().then_keyword_loc(),
        };
        if then.is_some() {
            return; // loc.begin is `then`
        }
        // Parser's loc.begin is a `;` following the condition (across blanks and newlines).
        let (cond, _, els) = parts(node);
        let src = cx.src;
        let cond_end = cond.location().end_offset();
        let blanks = src.text.as_bytes()[cond_end..].iter().take_while(|b| matches!(b, b' ' | b'\t' | b'\n')).count();
        let semi = cond_end + blanks;
        if src.text.as_bytes().get(semi) != Some(&b';') {
            return;
        }
        let newline = require_newline(node);
        let masgn = masgn_or_block(node);
        let else_begin_or_if =
            else_if(&els).is_some() || matches!(&els, Some(Else::Body(Some(b))) if b.is_begin());
        let template = if newline {
            "use a newline instead"
        } else if else_begin_or_if || masgn {
            "use `if/else` instead"
        } else {
            "use a ternary operator instead"
        };
        let keyword = if node.as_unless_node().is_some() { "unless" } else { "if" };
        let msg = format!("Do not use `{keyword} {};` - {template}.", node_text(&cond));
        let fix = if newline || masgn { Some(None) } else { replacement(src, node).map(Some) };
        // A raising corrector loses the offense and skips ignore_node.
        let Some(fix) = fix else { return };
        cx.state::<Vec<(usize, usize)>>().push((start, end));
        cx.add_fix(start, end, msg, |c| match &fix {
            None => c.replace(semi, semi + 1, "\n"),
            Some(r) => c.replace(start, end, r),
        });
    }
}
