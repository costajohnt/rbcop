//! Pieces of RuboCop's RangeHelp mixin.

use crate::source::Source;

/// `source_range(buffer, node.first_line, node.loc.column...node.loc.last_column)` for a node
/// spanning `start..end`: on the node's first line, from its start column to the column its last
/// line ends at (character columns). Used by the Bundler/Gemspec "duplicated" cops.
pub fn first_line_to_last_column(src: &Source, start: usize, end: usize) -> (usize, usize) {
    let last_column = src.column(end);
    let line_start = src.line_start(src.line_of(start));
    // ponytail: RuboCop raises (cop crash) when last_column < column on a multi-line node; we clamp.
    let stop = src.text[line_start..].char_indices().nth(last_column).map_or(src.text.len(), |(i, _)| line_start + i);
    (start, stop.max(start))
}

/// `range_with_surrounding_space(range, side:, newlines:)` (no continuations, no other whitespace):
/// widen over `[ \t]*` and then `\n*` on the chosen sides.
pub fn with_surrounding_space(
    src: &Source,
    start: usize,
    end: usize,
    left: bool,
    right: bool,
    newlines: bool,
) -> (usize, usize) {
    let b = src.text.as_bytes();
    let (mut s, mut e) = (start, end);
    if left {
        while s > 0 && matches!(b[s - 1], b' ' | b'\t') {
            s -= 1;
        }
        while newlines && s > 0 && b[s - 1] == b'\n' {
            s -= 1;
        }
    }
    if right {
        while e < b.len() && matches!(b[e], b' ' | b'\t') {
            e += 1;
        }
        while newlines && e < b.len() && b[e] == b'\n' {
            e += 1;
        }
    }
    (s, e)
}
