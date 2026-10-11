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
    b.is_some_and(|&b| helpers::parser_tokens::is_ruby_space(b))
}

/// At 3.3+ RuboCop lexes with Prism's translation lexer, which drops a one-line string content
/// token ending in a line continuation together with the token after it (its
/// `while token.type == :STRING_CONTENT` loop), so a `#{` right after `"a \` + newline is lost
/// (not in `%W`/`%I`, where that backslash escapes the newline).
fn lost_opening(cx: &Cx, open: usize) -> bool {
    let src = cx.src;
    if src.parser_gem {
        return false;
    }
    let Some(content) = src.tokens().iter().find(|t| t.end == open && t.kind == sys::PM_TOKEN_STRING_CONTENT) else {
        return false;
    };
    let value = src.slice(content.start, content.end);
    let Some(line) = value.strip_suffix('\n').filter(|l| !l.contains('\n')) else { return false };
    let backslashes = line.len() - line.trim_end_matches('\\').len();
    let word_array = cx.ancestors.len() >= 2
        && cx.ancestors[cx.ancestors.len() - 2]
            .as_array_node()
            .and_then(|a| a.opening_loc())
            .is_some_and(|l| l.as_slice().starts_with(b"%W") || l.as_slice().starts_with(b"%I"));
    backslashes % 2 == 1 && !word_array
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
            || inner.bytes().all(helpers::parser_tokens::is_ruby_space)
        {
            return;
        }
        let text = src.text.as_bytes();
        let (mut o, c) = ((open.start_offset(), open.end_offset()), (close.start_offset(), close.end_offset()));
        if lost_opening(cx, open.start_offset()) {
            // RuboCop's delimiters are then the first token inside and the `}`; with nothing else
            // between them the brackets count as empty.
            let inner: Vec<&Token> = src
                .tokens()
                .iter()
                .filter(|t| t.start >= o.1 && t.end <= c.0 && t.kind != sys::PM_TOKEN_IGNORED_NEWLINE)
                .collect();
            match inner.as_slice() {
                [first, _, ..] => o = (first.start, first.end),
                _ => return,
            }
        }
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
