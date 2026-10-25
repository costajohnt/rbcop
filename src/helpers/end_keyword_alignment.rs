//! RuboCop's EndKeywordAlignment mixin (EndAlignment, DefEndAlignment, BeginEndAlignment) and the
//! RangeHelp column helpers the alignment cops share.

use crate::cop::Cx;
use crate::source::Source;

/// `RangeHelp#effective_column`: the character column, minus one on a BOM-prefixed first line.
pub fn effective_column(src: &Source, off: usize) -> i64 {
    let col = src.column(off) as i64;
    if src.line_of(off) == 1 && src.text.starts_with('\u{feff}') { col - 1 } else { col }
}

/// `column_offset_between(base, range)`.
pub fn column_offset_between(src: &Source, base: usize, range: usize) -> i64 {
    effective_column(src, base) - effective_column(src, range)
}

/// `begins_its_line?(range)`: only (ASCII) whitespace before `off` on its line.
pub fn begins_its_line(src: &Source, off: usize) -> bool {
    src.text[src.line_start(src.line_of(off))..off].bytes().all(|b| matches!(b, b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r'))
}

/// `start_line_range(node)`: the node's first line from its first non-blank character to the end
/// of the line without trailing whitespace.
pub fn start_line_range(src: &Source, node_start: usize) -> (usize, usize) {
    let line = src.line_of(node_start);
    let text = src.lines.get(line - 1).copied().unwrap_or("");
    let base = src.line_start(line);
    let is_ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r');
    let first = text.find(|c: char| !is_ws(c)).unwrap_or(text.len());
    let last = text.trim_end_matches(is_ws).len().max(first);
    (base + first, base + last)
}

/// `check_end_kw_alignment(node, align_ranges)` once the node is known not to be ignored:
/// `end_kw` is `node.loc.end`, `ranges` the candidate (style, range) pairs.
/// `align_column` is where the autocorrection (`AlignmentCorrector.align_end`) puts the `end`.
pub fn check(cx: &mut Cx, end_kw: (usize, usize), ranges: &[(&str, (usize, usize))], style: &str, align_column: usize) {
    detect(cx, end_kw, ranges, style);
    if let Some(msg) = misalignment(cx.src, end_kw, ranges, style) {
        let src = cx.src;
        let tabs = super::alignment_corrector::Settings::of(cx).tabs;
        cx.add_fix(end_kw.0, end_kw.1, msg, |c| super::alignment_corrector::align_end(c, src, tabs, end_kw.0, align_column));
    }
}

/// `check_end_kw_alignment`'s style detection (`EnforcedStyleAlignWith`): the configured style if
/// its range is aligned with the `end`, else the styles whose ranges are.
pub fn detect(cx: &Cx, end_kw: (usize, usize), ranges: &[(&str, (usize, usize))], style: &str) {
    if !crate::autogen::on() {
        return;
    }
    let src = cx.src;
    let end_line = src.line_of(end_kw.0);
    let matching: Vec<&str> = ranges
        .iter()
        .filter(|(_, r)| src.line_of(r.0) == end_line || column_offset_between(src, r.0, end_kw.0) == 0)
        .map(|(s, _)| *s)
        .collect();
    if matching.contains(&style) {
        cx.correct_style_detected_for("EnforcedStyleAlignWith");
    } else {
        cx.style_detected_for("EnforcedStyleAlignWith", &matching);
    }
}

/// The offense message `check` would report, if any.
pub fn misalignment(src: &Source, end_kw: (usize, usize), ranges: &[(&str, (usize, usize))], style: &str) -> Option<String> {
    let end_line = src.line_of(end_kw.0);
    let matches = |r: (usize, usize)| src.line_of(r.0) == end_line || column_offset_between(src, r.0, end_kw.0) == 0;
    let &(_, align) = ranges.iter().find(|(s, _)| *s == style)?;
    if matches(align) {
        return None;
    }
    Some(format!(
        "`end` at {}, {} is not aligned with `{}` at {}, {}.",
        end_line,
        src.column(end_kw.0),
        src.slice(align.0, align.1),
        src.line_of(align.0),
        src.column(align.0)
    ))
}
