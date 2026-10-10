use crate::prelude::*;
use helpers::parentheses_corrector::{self, ParenFix};
use helpers::parser_parent::parser_parent;

pub const NAME: &str = "Style/RescueModifier";
pub static COP: RescueModifier = RescueModifier;
pub struct RescueModifier;

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// ParenthesesCorrector plan for the parser parent at `ancestors[idx]` when it ends with `)`.
fn paren_fix(cx: &Cx, idx: usize) -> Option<ParenFix> {
    let mut idx = idx;
    let anc = cx.ancestors;
    // A parenthesised 2+ statement body: parser's `begin` is the parentheses node.
    if anc[idx].as_statements_node().is_some() && idx > 0 && anc[idx - 1].as_parentheses_node().is_some() {
        idx -= 1;
    }
    let node = &anc[idx];
    let (open, close, children): (_, _, Vec<Node>) = if let Some(p) = node.as_parentheses_node() {
        let kids = match p.body() {
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![b],
            },
            None => vec![],
        };
        (p.opening_loc(), p.closing_loc(), kids)
    } else if let Some(c) = node.as_call_node() {
        let mut kids = call_args(&c);
        kids.extend(c.block().filter(|b| b.as_block_argument_node().is_some()));
        (c.opening_loc()?, c.closing_loc()?, kids)
    } else if let Some(s) = node.as_super_node() {
        let mut kids: Vec<Node> = s.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        kids.extend(s.block().filter(|b| b.as_block_argument_node().is_some()));
        (s.lparen_loc()?, s.rparen_loc()?, kids)
    } else if let Some(y) = node.as_yield_node() {
        (y.lparen_loc()?, y.rparen_loc()?, y.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default())
    } else {
        let d = node.as_defined_node()?;
        (d.lparen_loc()?, d.rparen_loc()?, vec![d.value()])
    };
    if close.as_slice() != b")" {
        return None;
    }
    let last = children.last();
    let heredoc = last.filter(|n| is_heredoc(n)).map(span);
    let me = span(node);
    let question = parser_parent(&anc[..idx]).and_then(|p| p.as_if_node()).and_then(|i| {
        (i.if_keyword_loc().is_none() && span(&i.predicate()) == me).then(|| i.then_keyword_loc().map(|q| q.start_offset()))?
    });
    Some(parentheses_corrector::plan(
        cx.src,
        (open.start_offset(), open.end_offset()),
        (close.start_offset(), close.end_offset()),
        last.map(span),
        heredoc,
        question,
    ))
}

impl Cop for RescueModifier {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_MODIFIER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `a rescue b rescue c`: parser reports the inner rescue (visited before the outer
        // resbody) first, so handle a chain from its outermost node, innermost first.
        let me = span(node);
        if cx.ancestors.last().and_then(|a| a.as_rescue_modifier_node()).is_some_and(|p| span(&p.expression()) == me) {
            return;
        }
        let mut chain = vec![node.as_rescue_modifier_node().unwrap()];
        while let Some(inner) = chain.last().unwrap().expression().as_rescue_modifier_node() {
            chain.push(inner);
        }
        let outer_fix = parser_parent(cx.ancestors).and_then(|p| {
            let s = span(p);
            let i = cx.ancestors.iter().rposition(|a| span(a) == s && kind::of(a) == kind::of(p)).unwrap();
            paren_fix(cx, i)
        });
        // Below 2.6 the parser gem reads `a, b = 1, 2 rescue x` as `(a, b = 1, 2) rescue x`.
        let masgn = (cx.config.target_ruby < 2.6)
            .then(|| cx.ancestors.last().and_then(|a| a.as_multi_write_node()))
            .flatten()
            .filter(|m| span(&m.value()) == me)
            .map(|m| m.location().start_offset());
        let mut outer_fix = Some(outer_fix);
        for (i, n) in chain.iter().enumerate().rev() {
            let (fix, masgn) = if i == 0 { (outer_fix.take().flatten(), masgn) } else { (None, None) };
            report(cx, n, fix, masgn);
        }
    }
}

fn report(cx: &mut Cx, n: &ruby_prism::RescueModifierNode, fix: Option<ParenFix>, masgn: Option<usize>) {
    {
        let src = cx.src;
        let (start, end) = span(&n.as_node());
        let start = masgn.unwrap_or(start);
        let parenthesized = fix.is_some();
        let width = helpers::alignment::configured_indentation_width(cx) as usize;
        let col = src.column(start);
        let (mut ind, mut off) = (" ".repeat(col + width), " ".repeat(col));
        if parenthesized {
            ind.pop();
            off.pop();
        }
        let op = n.expression();
        let op_span = (start, op.location().end_offset());
        let wrap_array = masgn.is_none() && op.as_array_node().is_some_and(|a| a.opening_loc().is_none());
        let heredoc_end = op.as_call_node().and_then(|c| {
            call_args(&c).iter().rev().find_map(|a| helpers::heredoc::parts(a).map(|(_, e, _)| e))
        });
        let clause = format!("\n{off}rescue\n{ind}{}\n{off}end", node_text(&n.rescue_expression()));
        cx.add_fix(start, end, "Avoid using `rescue` in its modifier form.", |c| {
            if wrap_array {
                c.wrap(op_span.0, op_span.1, "[", "]");
            }
            c.remove(op_span.1, end);
            c.insert_before(op_span.0, op_span.1, &format!("begin\n{ind}"));
            let (s, e) = heredoc_end.unwrap_or(op_span);
            c.insert_after(s, e, &clause);
            if let Some(f) = &fix {
                parentheses_corrector::apply(c, f);
            }
        });
    }
}
