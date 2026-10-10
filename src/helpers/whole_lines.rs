//! RangeHelp#range_by_whole_lines.

/// `range_by_whole_lines(range, include_final_newline:)` on byte offsets: from the start of the
/// range's first line to the end of its last line (plus the newline), clipped to the buffer.
pub fn range_by_whole_lines(text: &str, start: usize, end: usize, include_final_newline: bool) -> (usize, usize) {
    let s = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let mut e = text[end..].find('\n').map_or(text.len(), |i| end + i);
    if include_final_newline {
        e = (e + 1).min(text.len());
    }
    (s, e)
}
