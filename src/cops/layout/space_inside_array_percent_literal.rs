use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideArrayPercentLiteral";
pub static COP: SpaceInsideArrayPercentLiteral = SpaceInsideArrayPercentLiteral;
pub struct SpaceInsideArrayPercentLiteral;

// `(?:[\S&&[^\\]](?:\\ )*)( {2,})(?=\S)` without the lookahead: the trailing \S is matched but the
// next search starts right after the spaces (Ruby \s is ASCII-only).
static MULTIPLE_SPACES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^ \t\n\x0B\x0C\r\\](?:\\ )*( {2,})[^ \t\n\x0B\x0C\r]").unwrap());

impl Cop for SpaceInsideArrayPercentLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        let (Some(open), Some(close)) = (array.opening_loc(), array.closing_loc()) else { return };
        let o = open.as_slice();
        if o.len() < 2 || !matches!(&o[..o.len() - 1], b"%i" | b"%I" | b"%w" | b"%W") {
            return;
        }
        let (start, end) = (open.end_offset(), close.start_offset());
        let contents = cx.src.slice(start, end);
        let mut pos = 0;
        while let Some(c) = MULTIPLE_SPACES.captures_at(contents, pos) {
            let g = c.get(1).unwrap();
            cx.add(start + g.start(), start + g.end(), "Use only a single space inside array percent literal.");
            pos = g.end();
        }
    }
}
