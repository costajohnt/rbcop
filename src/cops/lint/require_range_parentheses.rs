use crate::prelude::*;

pub const NAME: &str = "Lint/RequireRangeParentheses";
pub static COP: RequireRangeParentheses = RequireRangeParentheses;
pub struct RequireRangeParentheses;

/// The parser parent of the visited node is a `begin`: parentheses, or a list of 2+ statements
/// other than the direct body of a plain `begin..end` (kwbegin).
fn parent_is_begin(ancestors: &[Node]) -> bool {
    let Some(i) = helpers::parser_parent::parser_parent_index(ancestors, ancestors.len()) else { return false };
    let p = &ancestors[i];
    if p.as_parentheses_node().is_some() {
        return true;
    }
    if p.as_statements_node().is_none() {
        return false;
    }
    let kwbegin = i > 0
        && ancestors[i - 1].as_begin_node().is_some_and(|b| {
            b.begin_keyword_loc().is_some()
                && b.rescue_clause().is_none()
                && b.ensure_clause().is_none()
                && b.statements().is_some_and(|s| s.location().start_offset() == p.location().start_offset())
        });
    !kwbegin
}

impl Cop for RequireRangeParentheses {
    fn kinds(&self) -> &'static [usize] {
        &[kind::RANGE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let range = node.as_range_node().unwrap();
        let (Some(left), Some(right)) = (range.left(), range.right()) else { return };
        if parent_is_begin(cx.ancestors) {
            return;
        }
        let op = range.operator_loc();
        if cx.src.line_of(op.start_offset()) == cx.src.line_of(right.location().start_offset()) {
            return;
        }
        let msg = format!(
            "Wrap the range literal `{}{}` in parentheses to avoid confusion with an endless range.",
            node_text(&left),
            text(&op)
        );
        let l = node.location();
        cx.add(l.start_offset(), l.end_offset(), msg);
    }
}
