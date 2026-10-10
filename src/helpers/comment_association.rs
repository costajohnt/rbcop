//! A slice of parser's `Comment::Associator` (`processed_source.ast_with_comments[node]`) for
//! statements: the leading comments a statement collects. Interior comments are always taken by
//! the statement or its descendants and decorating comments sit on its own lines, so for whole-line
//! ranges (`range_with_comments_and_lines`) the leading comments are all that matter.

use crate::source::{Comment, Source};
use regex::Regex;
use std::sync::LazyLock;

static MAGIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#\s*(?:-\*-\s*(?:frozen_string_literal|warn_indent|warn_past_scope):.*-\*-$|\s*(?:frozen_string_literal|warn_indent|warn_past_scope):)")
        .unwrap()
});
static ENCODING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\s#](en)?coding\s*[:=]\s*[A-Za-z0-9_-]").unwrap());

/// Number of leading comments `advance_through_directives` skips (shebang, magic, encoding).
fn skipped_directives(src: &Source) -> usize {
    let text = |i: usize| src.comments.get(i).map(|c| src.slice(c.start, c.end));
    let mut i = 0;
    if text(i).is_some_and(|t| t.starts_with("#!")) {
        i += 1;
    }
    if text(i).is_some_and(|t| MAGIC.is_match(t)) {
        i += 1;
    }
    if text(i).is_some_and(|t| ENCODING.is_match(t)) {
        i += 1;
    }
    i
}

/// Leading comments of a statement starting at `start`: comments ending before it that come after
/// `after` (byte offset) and lie on a line below `after_line` (comments on the previous sibling's
/// last line decorate that sibling).
pub fn leading<'a>(src: &'a Source, start: usize, after: usize, after_line: Option<usize>) -> Vec<&'a Comment> {
    let skip = skipped_directives(src);
    // Comments are sorted and disjoint: binary-search the window instead of scanning them all.
    let lo = src.comments.partition_point(|c| c.start < after).max(skip);
    let hi = src.comments.partition_point(|c| c.end <= start);
    src.comments
        .get(lo..hi)
        .unwrap_or_default()
        .iter()
        .filter(|c| after_line.is_none_or(|l| src.line_of(c.start) > l))
        .collect()
}
