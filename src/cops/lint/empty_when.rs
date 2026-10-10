use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyWhen";
pub static COP: EmptyWhen = EmptyWhen;
pub struct EmptyWhen;

impl Cop for EmptyWhen {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_node().unwrap();
        let whens: Vec<_> = case.conditions().iter().collect();
        for (i, w) in whens.iter().enumerate() {
            let when = w.as_when_node().unwrap();
            if when.statements().is_some() {
                continue;
            }
            // CommentsHelp#find_end_line: the right sibling's line, else the case's `end` line.
            let next = match whens.get(i + 1) {
                Some(n) => Some(n.location().start_offset()),
                None => case
                    .else_clause()
                    .and_then(|e| e.statements())
                    .map(|s| s.location().start_offset()),
            };
            let end_line = cx.src.line_of(next.unwrap_or(case.end_keyword_loc().start_offset()));
            let loc = w.location();
            if helpers::comments_help::allow_comments(cx, cx.src.line_of(loc.start_offset()), end_line) {
                continue;
            }
            // Parser's `when` without a body ends at its last condition, not at `then`.
            let end = when.conditions().iter().last().map_or(loc.end_offset(), |c| c.location().end_offset());
            cx.add(loc.start_offset(), end, "Avoid `when` branches without a body.");
        }
    }
}
