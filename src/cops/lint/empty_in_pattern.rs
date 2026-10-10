use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyInPattern";
pub static COP: EmptyInPattern = EmptyInPattern;
pub struct EmptyInPattern;

impl Cop for EmptyInPattern {
    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_MATCH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_match_node().unwrap();
        let branches: Vec<_> = case.conditions().iter().collect();
        for (i, b) in branches.iter().enumerate() {
            let branch = b.as_in_node().unwrap();
            if branch.statements().is_some() {
                continue;
            }
            // CommentsHelp#find_end_line: the right sibling's line (an empty `else` is parser's
            // `empty_else`, located at the keyword), else the case's `end` line.
            let next = match branches.get(i + 1) {
                Some(n) => n.location().start_offset(),
                None => match case.else_clause() {
                    Some(e) => e.statements().map_or(e.else_keyword_loc().start_offset(), |s| s.location().start_offset()),
                    None => case.end_keyword_loc().start_offset(),
                },
            };
            let start = b.location().start_offset();
            if helpers::comments_help::allow_comments(cx, cx.src.line_of(start), cx.src.line_of(next)) {
                continue;
            }
            // Parser's bodiless `in` ends at its pattern (guard included), not at `then`.
            cx.add(start, branch.pattern().location().end_offset(), "Avoid `in` branches without a body.");
        }
    }
}
