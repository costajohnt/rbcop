//! Deprecated: use `helpers::alignment` (and `alignment_corrector` for corrections). Kept as thin
//! re-exports so branches written against this earlier port still compile.

use crate::cop::Cx;
use crate::helpers::alignment_corrector::{Settings, correct};
use crate::source::Source;
use ruby_prism::Node;

pub use super::alignment::{
    begins_its_line, blank, cop_enabled, display_column, display_width, enabled_cop_str, first_non_space,
};

/// Deprecated: `alignment::configured_indentation_width`.
pub fn indentation_width(cx: &Cx) -> i64 {
    super::alignment::configured_indentation_width(cx)
}

/// Deprecated: report with `cx.add_fix` + `alignment_corrector::correct`. Whether
/// AlignmentCorrector (spaces style) edits anything on `start..end`.
pub fn corrector_edits(src: &Source, start: usize, end: usize, delta: i64, node: Option<&Node>) -> bool {
    let mut c = crate::autocorrect::corrector::Corrector::new(src.text.into());
    correct(&mut c, src, Settings { tabs: false, width: 2 }, start, end, node, delta, false);
    !c.is_empty()
}
