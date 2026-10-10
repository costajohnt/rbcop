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

/// `last_heredoc_argument(node.body)`: the terminator range (`loc.heredoc_end`, without the
/// newline) of the last single-line `str` heredoc argument of a send, searching receivers too.
fn last_heredoc_end(body: Option<&Node>) -> Option<(usize, usize)> {
    let mut node = match body.and_then(|b| b.as_statements_node()) {
        Some(s) if s.body().len() == 1 => s.body().iter().next(),
        _ => None,
    };
    while let Some(call) = node.as_ref().and_then(|n| n.as_call_node()) {
        // A call with a literal block is a parser `block`, not `send`.
        if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return None;
        }
        let heredoc = send_args(&call).into_iter().rev().find(|a| is_heredoc(a) && a.as_string_node().is_some() && !is_parser_dstr(a));
        if let Some(h) = heredoc {
            let close = h.as_string_node().unwrap().closing_loc().unwrap();
            let end = close.end_offset() - usize::from(close.as_slice().ends_with(b"\n"));
            return Some((close.start_offset(), end));
        }
        node = call.receiver();
    }
    None
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
        if src.line_of(start) == src.line_of(end) || helpers::alignment::begins_its_line(src, end) {
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
        let close_end = closing.end_offset();
        let replacement = format!("\n{}", src.slice(last_end, close_end).trim_start());
        let heredoc = last_heredoc_end(body.as_ref());
        let msg = format!("Expression at {line}, {column} should be on its own line.");
        cx.add_fix(end, close_end, msg, |c| match heredoc {
            Some((hs, he)) => {
                c.remove(last_end, close_end);
                c.insert_after(hs, he, &replacement);
            }
            None => c.replace(last_end, close_end, &replacement),
        });
    }
}
