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

/// Deprecated: use `range_with_surrounding_space`. Kept so other branches still compile.
pub fn with_surrounding_space(
    src: &Source,
    start: usize,
    end: usize,
    left: bool,
    right: bool,
    newlines: bool,
) -> (usize, usize) {
    let side = match (left, right) {
        (true, false) => Side::Left,
        (false, true) => Side::Right,
        (true, true) => Side::Both,
        (false, false) => return (start, end),
    };
    range_with_surrounding_space(src.text, start, end, Surrounding { side, newlines, ..Default::default() })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Both,
    Left,
    Right,
}

/// Keyword arguments of `range_with_surrounding_space` (defaults as in RuboCop).
#[derive(Clone, Copy)]
pub struct Surrounding {
    pub side: Side,
    pub newlines: bool,
    pub whitespace: bool,
    pub continuations: bool,
}

impl Default for Surrounding {
    fn default() -> Self {
        Surrounding { side: Side::Both, newlines: true, whitespace: false, continuations: false }
    }
}

/// `range_with_surrounding_space(range, side:, newlines:, whitespace:, continuations:)`.
pub fn range_with_surrounding_space(src: &str, start: usize, end: usize, opts: Surrounding) -> (usize, usize) {
    let left = opts.side != Side::Right;
    let right = opts.side != Side::Left;
    let b = if left { final_pos(src, start as isize, -1, opts) } else { start };
    let e = if right { final_pos(src, end as isize, 1, opts) } else { end };
    (b, e)
}

fn final_pos(src: &str, pos: isize, step: isize, o: Surrounding) -> usize {
    let blank = |c: u8| c == b' ' || c == b'\t';
    let pos = move_pos(src, pos, step, true, blank);
    let pos = move_pos_str(src, pos, step, o.continuations, b"\\\n");
    let pos = move_pos(src, pos, step, o.newlines, |c| c == b'\n');
    move_pos(src, pos, step, o.whitespace, |c| c.is_ascii_whitespace() || c == 0x0b) as usize
}

/// Ruby's `src[i]`: a negative index counts from the end (RuboCop relies on this at offset 0).
fn at(src: &[u8], i: isize) -> Option<u8> {
    let i = if i < 0 { src.len() as isize + i } else { i };
    if i < 0 { None } else { src.get(i as usize).copied() }
}

fn move_pos(src: &str, mut pos: isize, step: isize, cond: bool, matches: impl Fn(u8) -> bool) -> isize {
    let offset = if step == -1 { -1 } else { 0 };
    while cond && at(src.as_bytes(), pos + offset).is_some_and(&matches) {
        pos += step;
    }
    pos.max(0)
}

fn move_pos_str(src: &str, mut pos: isize, step: isize, cond: bool, needle: &[u8]) -> isize {
    let size = needle.len() as isize;
    let offset = if step == -1 { -size } else { 0 };
    let bytes = src.as_bytes();
    while cond && (0..size).all(|k| at(bytes, pos + offset + k) == Some(needle[k as usize])) {
        pos += size * step;
    }
    pos.max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surrounding_space() {
        let s = "a  # x  \n\nb";
        assert_eq!(range_with_surrounding_space(s, 3, 6, Surrounding { side: Side::Right, ..Default::default() }), (3, 10));
        assert_eq!(range_with_surrounding_space(s, 3, 6, Surrounding::default()), (1, 10));
        let o = Surrounding { newlines: false, ..Default::default() };
        assert_eq!(range_with_surrounding_space(s, 3, 6, o), (1, 8));
    }
}
