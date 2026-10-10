//! Pieces of RuboCop's CommentsHelp mixin. The caller computes `find_end_line` for its node shape.

use crate::cop::Cx;

/// `processed_source.each_comment_in_lines(start_line...end_line).any?` (1-based lines).
pub fn contains_comments(cx: &Cx, start_line: usize, end_line: usize) -> bool {
    cx.src.comments.iter().any(|c| (start_line..end_line).contains(&cx.src.line_of(c.start)))
}

/// `comments_contain_disables?`: a disabled range for this cop covers `start_line...end_line`
/// or lies inside it (Range#cover? semantics).
pub fn comments_contain_disables(cx: &Cx, start_line: usize, end_line: usize) -> bool {
    let Some(ranges) = cx.facts.disabled.get(cx.cop) else { return false };
    let (s, e) = (start_line as i64, end_line as i64);
    ranges.iter().any(|&(a, b)| (e > s && a <= s && e - 1 <= b) || (s <= a && b < e))
}

/// `cop_config['AllowComments'] && contains_comments?(node) && !comments_contain_disables?(node, name)`.
pub fn allow_comments(cx: &Cx, start_line: usize, end_line: usize) -> bool {
    cx.cfg.bool("AllowComments").unwrap_or(false)
        && contains_comments(cx, start_line, end_line)
        && !comments_contain_disables(cx, start_line, end_line)
}
