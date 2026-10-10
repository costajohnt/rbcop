use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideStringInterpolation";
pub static COP: SpaceInsideStringInterpolation = SpaceInsideStringInterpolation;
pub struct SpaceInsideStringInterpolation;

fn blank(b: Option<&u8>) -> bool {
    matches!(b, Some(b' ' | b'\t'))
}

impl Cop for SpaceInsideStringInterpolation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::EMBEDDED_STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let e = node.as_embedded_statements_node().unwrap();
        let (open, close) = (e.opening_loc(), e.closing_loc());
        let src = cx.src;
        // empty_brackets?: no token between `#{` and `}` (`#{ ;}` has one, Prism drops it).
        let inner = src.slice(open.end_offset(), close.start_offset());
        if src.line_of(open.start_offset()) != src.line_of(close.start_offset())
            || inner.bytes().all(|b| b == b' ' || b == b'\t')
        {
            return;
        }
        let text = src.text.as_bytes();
        let left_space = blank(text.get(open.end_offset()));
        let right_space = blank(text.get(close.start_offset().wrapping_sub(1)));
        // The first offense's corrector fixes both sides; the second one finds the node ignored.
        let mut correctable = true;
        if cx.cfg.str("EnforcedStyle") == Some("space") {
            let msg = "Use space inside string interpolation.";
            if !left_space {
                cx.add(open.start_offset(), open.end_offset(), msg);
                correctable = false;
            }
            if !right_space {
                cx.add_with(close.start_offset(), close.end_offset(), msg, correctable);
            }
        } else {
            let msg = "Do not use space inside string interpolation.";
            if left_space {
                let s = open.end_offset();
                let n = text[s..].iter().take_while(|&&b| b == b' ' || b == b'\t').count();
                cx.add(s, s + n, msg);
                correctable = false;
            }
            if right_space {
                let e = close.start_offset();
                let n = text[..e].iter().rev().take_while(|&&b| b == b' ' || b == b'\t').count();
                cx.add_with(e - n, e, msg, correctable);
            }
        }
    }
}
