//! RuboCop's RangeHelp mixin (lib/rubocop/cop/mixin/range_help.rb). Byte offsets.

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
