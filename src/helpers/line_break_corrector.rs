//! RuboCop's TrailingBody mixin and LineBreakCorrector (class/module/sclass/def bodies).

use crate::prelude::*;
use crate::source::Source;

/// TrailingBody#first_part_of on a Prism class/module/def `body()`: the first statement of a
/// parser `begin`, otherwise the parser body range (`ast::body_range` for rescue/ensure bodies).
pub fn first_part_of(body: &Node) -> (usize, usize) {
    if let Some(first) = body.as_statements_node().and_then(|s| s.body().iter().next()) {
        let l = first.location();
        return (l.start_offset(), l.end_offset());
    }
    body_range(body)
}

/// TrailingBody#trailing_body?: a body, a multi-line node, and the body starting on its first line.
pub fn trailing_body(src: &Source, node: &Node, body: Option<&Node>) -> bool {
    let Some(body) = body else { return false };
    let l = node.location();
    let first = src.line_of(l.start_offset());
    first != src.line_of(l.end_offset()) && first == src.line_of(first_part_of(body).0)
}

/// LineBreakCorrector.break_line_before: newline plus `keyword column + steps * width` spaces.
pub fn break_line_before(c: &mut Corrector, range: (usize, usize), keyword_col: usize, width: i64, steps: i64) {
    let pad = (keyword_col as i64 + steps * width).max(0) as usize;
    c.insert_before(range.0, range.1, &format!("\n{}", " ".repeat(pad)));
}

/// LineBreakCorrector.move_comment: the comment on the node's first line goes above the node.
pub fn move_comment(c: &mut Corrector, src: &Source, node: (usize, usize), keyword_col: usize) {
    let Some(cm) = src.comment_on_line(src.line_of(node.0)) else { return };
    let text = &src.text[cm.start..cm.end];
    c.insert_before(node.0, node.1, &format!("{text}\n{}", " ".repeat(keyword_col)));
    c.remove(cm.start, cm.end);
}

/// LineBreakCorrector.correct_trailing_body for a class/module/sclass/def `node` whose keyword
/// starts at `keyword` and whose Prism body is `body`.
pub fn correct_trailing_body(c: &mut Corrector, src: &Source, node: &Node, keyword: usize, body: &Node, width: i64) {
    let l = node.location();
    let range = (l.start_offset(), l.end_offset());
    let col = src.column(keyword);
    let first = first_part_of(body);
    break_line_before(c, first, col, width, 1);
    move_comment(c, src, range, col);
    // remove_semicolon: a `;` after the node start, on the body's line, left of the body.
    let (body_line, body_col) = (src.line_of(first.0), src.column(first.0));
    let semi = src.tokens().iter().find(|t| {
        t.kind == sys::PM_TOKEN_SEMICOLON
            && t.end > range.0
            && src.line_of(t.start) == body_line
            && src.column(t.start) < body_col
    });
    if let Some(t) = semi {
        c.remove(t.start, t.end);
    }
}
