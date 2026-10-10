use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsidePercentLiteralDelimiters";
pub static COP: SpaceInsidePercentLiteralDelimiters = SpaceInsidePercentLiteralDelimiters;
pub struct SpaceInsidePercentLiteralDelimiters;

const MSG: &str = "Do not use spaces inside percent literal delimiters.";

impl Cop for SpaceInsidePercentLiteralDelimiters {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE, kind::X_STRING_NODE, kind::INTERPOLATED_X_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (open, close, types): (_, _, &[&[u8]]) = match kind::of(node) {
            kind::ARRAY_NODE => {
                let a = node.as_array_node().unwrap();
                (a.opening_loc(), a.closing_loc(), &[b"%i", b"%I", b"%w", b"%W"])
            }
            kind::X_STRING_NODE => {
                let x = node.as_x_string_node().unwrap();
                (Some(x.opening_loc()), Some(x.closing_loc()), &[b"%x"])
            }
            _ => {
                let x = node.as_interpolated_x_string_node().unwrap();
                (Some(x.opening_loc()), Some(x.closing_loc()), &[b"%x"])
            }
        };
        let (Some(open), Some(close)) = (open, close) else { return };
        let o = open.as_slice();
        if !o.starts_with(b"%") || !types.contains(&&o[..o.len() - 1]) {
            return;
        }
        let (start, end) = (open.end_offset(), close.start_offset());
        let body = cx.src.slice(start, end);
        // `range.source.strip.empty?` (Ruby strip: ASCII whitespace and NUL).
        if !body.is_empty() && body.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r' | 0)) {
            cx.add_fix(start, end, MSG, |c| c.remove(start, end));
        }
        let loc = node.location();
        if cx.src.line_of(loc.start_offset()) != cx.src.line_of(loc.end_offset()) {
            return;
        }
        // BEGIN_REGEX /\A( +)/
        let lead = body.bytes().take_while(|&b| b == b' ').count();
        if lead > 0 {
            cx.add_fix(start, start + lead, MSG, |c| c.remove(start, start + lead));
        }
        // END_REGEX /(?<!\\)( +)\z/: leftmost start of the trailing space run not preceded by `\`.
        let trail = body.trim_end_matches(' ').len();
        let s = if trail > 0 && body.as_bytes()[trail - 1] == b'\\' { trail + 1 } else { trail };
        if s < body.len() {
            cx.add_fix(start + s, end, MSG, |c| c.remove(start + s, end));
        }
    }
}
