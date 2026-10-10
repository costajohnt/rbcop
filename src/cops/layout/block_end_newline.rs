use crate::prelude::*;

pub const NAME: &str = "Layout/BlockEndNewline";
pub static COP: BlockEndNewline = BlockEndNewline;
pub struct BlockEndNewline;

/// End offset of a parser-gem block body (Prism StatementsNode, or an implicit BeginNode holding
/// rescue/else/ensure clauses, which parser nests as `ensure(rescue(body, resbody..., else))`).
fn body_end(body: &Node) -> usize {
    let Some(b) = body.as_begin_node() else { return body.location().end_offset() };
    let stmts_end = |s: Option<ruby_prism::StatementsNode>| s.map(|s| s.location().end_offset());
    if let Some(e) = b.ensure_clause() {
        return stmts_end(e.statements()).unwrap_or(e.ensure_keyword_loc().end_offset());
    }
    if let Some(e) = b.else_clause() {
        return stmts_end(e.statements()).unwrap_or(e.else_keyword_loc().end_offset());
    }
    if let Some(mut r) = b.rescue_clause() {
        while let Some(next) = r.subsequent() {
            r = next;
        }
        let ends = [
            stmts_end(r.statements()),
            r.reference().map(|n| n.location().end_offset()),
            r.then_keyword_loc().map(|l| l.end_offset()),
            r.exceptions().iter().last().map(|n| n.location().end_offset()),
            Some(r.keyword_loc().end_offset()),
        ];
        return ends.into_iter().flatten().max().unwrap();
    }
    stmts_end(b.statements()).unwrap_or(body.location().start_offset())
}

impl Cop for BlockEndNewline {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (body, params, opening, closing) = if let Some(b) = node.as_block_node() {
            (b.body(), b.parameters(), b.opening_loc(), b.closing_loc())
        } else {
            let l = node.as_lambda_node().unwrap();
            (l.body(), l.parameters(), l.opening_loc(), l.closing_loc())
        };
        let src = cx.src;
        // BlockNode#single_line?: `{`/`do` and `}`/`end` on one line.
        let start = opening.start_offset();
        let end = closing.start_offset();
        if src.line_of(start) == src.line_of(end) || helpers::alignment_mixin::begins_its_line(src, end) {
            return;
        }
        let last_end = match (&body, &params) {
            (Some(b), _) => body_end(b),
            // numbered/it parameters have no source in parser; RuboCop needs a body there.
            (None, Some(p)) if p.as_block_parameters_node().is_some() => p.location().end_offset(),
            _ => return, // RuboCop crashes (args without a source range)
        };
        if src.slice(last_end, end).trim_start().starts_with(';') {
            return;
        }
        let line = src.line_of(end);
        let column = src.column(end) + 1;
        cx.add(end, closing.end_offset(), format!("Expression at {line}, {column} should be on its own line."));
    }
}
