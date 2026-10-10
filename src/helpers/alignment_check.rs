//! Deprecated: use `helpers::alignment` (and `alignment_corrector` for corrections). Kept as thin
//! re-exports so branches written against this earlier port still compile.

use crate::cop::Cx;
use ruby_prism::Node;

pub use super::alignment::{
    Item, begins_its_line, check_alignment, configured_indentation_width, display_column, display_width,
};
pub use super::alignment_corrector::taboo_ranges;

/// Deprecated: `alignment::first_non_space(line).unwrap_or(0)`.
pub fn first_non_space(line: &str) -> usize {
    super::alignment::first_non_space(line).unwrap_or(0)
}

/// Deprecated: report with `cx.add_fix` + `alignment_corrector::correct`. Whether
/// AlignmentCorrector edits anything on `start..end`.
pub fn corrects(cx: &Cx, start: usize, end: usize, node: Option<&Node>, delta: i64) -> bool {
    super::alignment_corrector::edits(cx, start, end, node, delta)
}
