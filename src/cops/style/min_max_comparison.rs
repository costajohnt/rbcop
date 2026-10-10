use crate::helpers::if_node::{Else, is_elsif, parts};
use crate::prelude::*;

pub const NAME: &str = "Style/MinMaxComparison";
pub static COP: MinMaxComparison = MinMaxComparison;
pub struct MinMaxComparison;

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

impl Cop for MinMaxComparison {
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
        let (cond, if_body, els) = parts(node);
        // `{(send $_ $OP $_) (begin (send $_ $OP $_))}`
        let cmp = match cond.as_parentheses_node() {
            Some(p) => p.body().and_then(|b| b.as_statements_node()).and_then(|s| {
                let v: Vec<Node> = s.body().iter().collect();
                (v.len() == 1).then(|| v.into_iter().next().unwrap())
            }),
            None => Some(cond),
        };
        let Some(cmp) = cmp.and_then(|c| c.as_call_node()) else { return };
        let op = method_name(&cmp);
        if !matches!(op, ">" | ">=" | "<" | "<=") || is_csend(&cmp) || cmp.block().is_some() {
            return;
        }
        let Some(lhs) = cmp.receiver() else { return };
        let args = call_args(&cmp);
        let [rhs] = args.as_slice() else { return };
        let if_r = if_body.as_ref().map(|b| b.range());
        let else_r = match &els {
            Some(Else::Body(b)) => b.as_ref().map(|b| b.range()),
            Some(Else::Elsif(n)) => Some((n.location().start_offset(), helpers::if_node::parser_end(n))),
            None => None,
        };
        let (if_r, else_r) = if node.as_unless_node().is_some() { (else_r, if_r) } else { (if_r, else_r) };
        let (Some(if_r), Some(else_r)) = (if_r, else_r) else { return };
        let src = cx.src;
        let eq = |a: (usize, usize), b: (usize, usize)| helpers::node_equality::same(src, a, b);
        let greater = matches!(op, ">" | ">=");
        let method = if eq(range(&lhs), if_r) && eq(range(rhs), else_r) {
            if greater { "max" } else { "min" }
        } else if eq(range(&lhs), else_r) && eq(range(rhs), if_r) {
            if greater { "min" } else { "max" }
        } else {
            return;
        };
        let replacement = format!("[{}, {}].{method}", node_text(&lhs), node_text(rhs));
        let start = node.location().start_offset();
        let end = helpers::if_node::parser_end(node);
        let msg = format!("Use `{replacement}` instead.");
        if is_elsif(node) {
            let else_kw = node.as_if_node().unwrap().subsequent().and_then(|s| s.as_else_node()).map(|e| e.else_keyword_loc().start_offset());
            let Some(else_kw) = else_kw else { return };
            cx.add_fix(start, end, msg, |c| {
                c.remove(start, else_kw);
                c.replace(else_r.0, else_r.1, &replacement);
            });
        } else {
            cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
        }
    }
}
