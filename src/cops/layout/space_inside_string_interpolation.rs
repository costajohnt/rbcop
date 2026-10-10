use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideStringInterpolation";
pub static COP: SpaceInsideStringInterpolation = SpaceInsideStringInterpolation;
pub struct SpaceInsideStringInterpolation;

/// SurroundingSpace SINGLE_SPACE_REGEXP.
fn blank(b: Option<&u8>) -> bool {
    matches!(b, Some(b' ' | b'\t'))
}

/// Token#space_after? / space_before? (`\s`).
fn ws(b: Option<&u8>) -> bool {
    matches!(b, Some(b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
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
        let (o, c) = ((open.start_offset(), open.end_offset()), (close.start_offset(), close.end_offset()));
        let left_space = blank(text.get(o.1));
        let right_space = blank(text.get(c.0.wrapping_sub(1)));
        let lead = text[o.1..].iter().take_while(|&&b| b == b' ' || b == b'\t').count();
        let trail = text[..c.0].iter().rev().take_while(|&&b| b == b' ' || b == b'\t').count();
        let space_style = cx.cfg.str("EnforcedStyle") == Some("space");
        let (after, before) = (ws(text.get(o.1)), ws(text.get(c.0.wrapping_sub(1))));
        // SpaceCorrector.add_space / remove_space. The first offense's block corrects both sides
        // and ignores the node, so a second offense's block does nothing.
        let mut done = false;
        let mut fix = |k: &mut Corrector| {
            if done {
                return;
            }
            done = true;
            if space_style {
                if !after {
                    k.insert_after(o.0, o.1, " ");
                }
                if !before {
                    k.insert_before(c.0, c.1, " ");
                }
            } else {
                if after {
                    k.remove(o.1, o.1 + lead);
                }
                if before {
                    k.remove(c.0 - trail, c.0);
                }
            }
        };
        let (msg, ranges) = if space_style {
            ("Use space inside string interpolation.", [(!left_space).then_some(o), (!right_space).then_some(c)])
        } else {
            (
                "Do not use space inside string interpolation.",
                [left_space.then_some((o.1, o.1 + lead)), right_space.then_some((c.0 - trail, c.0))],
            )
        };
        for (s, e) in ranges.into_iter().flatten() {
            cx.add_fix(s, e, msg, &mut fix);
        }
    }
}
