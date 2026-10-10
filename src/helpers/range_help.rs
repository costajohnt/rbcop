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
