//! RuboCop's SurroundingSpace mixin and SpaceCorrector (SpaceInside*Brackets/Braces cops).

use crate::cop::{Corrector, Cx};
use crate::helpers::parser_tokens::{space_after, space_before};
use crate::source::{Source, Token};
use std::cell::Cell;

pub const NO_SPACE_COMMAND: &str = "Do not use";
pub const SPACE_COMMAND: &str = "Use";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    None,
}

/// `SINGLE_SPACE_REGEXP = /[ \t]/`: deliberately not Ruby's `\s`.
fn blank(c: Option<&u8>) -> bool {
    matches!(c, Some(b' ' | b'\t'))
}

/// `reposition(src, pos, step, include_newlines:)`.
fn reposition(src: &[u8], mut pos: usize, step: isize, newlines: bool) -> usize {
    loop {
        let c = if step < 0 {
            // Ruby's src[-1] is the last character.
            if pos == 0 { src.last() } else { src.get(pos - 1) }
        } else {
            src.get(pos)
        };
        if !(blank(c) || (newlines && c == Some(&b'\n'))) {
            return pos;
        }
        if step < 0 {
            if pos == 0 {
                return 0;
            }
            pos -= 1;
        } else {
            pos += 1;
        }
    }
}

/// `side_space_range(range:, side:, include_newlines:)`.
pub fn side_space_range(src: &Source, start: usize, end: usize, side: Side, newlines: bool) -> (usize, usize) {
    let b = src.text.as_bytes();
    match side {
        Side::Left => (reposition(b, start, -1, newlines), start),
        Side::Right => (end, reposition(b, end, 1, newlines)),
        Side::None => (start, end),
    }
}

/// `extra_space?(token, side)`: a space or tab right after (`:left`) / before (`:right`) the token.
pub fn extra_space(src: &Source, token: Option<&Token>, side: Side) -> bool {
    let Some(t) = token else { return false };
    let b = src.text.as_bytes();
    if side == Side::Left {
        space_after(src, t) && blank(b.get(t.end))
    } else {
        space_before(src, t) && blank(b.get(t.start.saturating_sub(1)))
    }
}

/// `space_offense`: the first offense for a node runs the node's full correction, later ones
/// (the node is then ignored) correct nothing. `ignored` holds the nodes' ranges.
pub fn space_offense(
    cx: &mut Cx,
    ignored: &mut Vec<(usize, usize)>,
    node: (usize, usize),
    (token, side, command): Planned,
    message: &str,
    correct: &dyn Fn(&mut Corrector),
) {
    let (s, e) = side_space_range(cx.src, token.start, token.end, side, false);
    let is_ignored = ignored.contains(&node);
    let ran = Cell::new(false);
    cx.add_fix(s, e, message.replace("%<command>s", command), |c| {
        ran.set(true);
        if !is_ignored {
            correct(c);
        }
    });
    if ran.get() {
        ignored.push(node);
    }
}

/// Offense list entry produced by `no_space_offenses` / `space_offenses`: (token, side, command).
pub type Planned = (Token, Side, &'static str);

/// `autocorrect_with_disable_uncorrectable?`: then only the left side of a pair is reported
/// (unless the left side is fine), so one todo comment covers both.
pub fn uncorrectable(cx: &crate::cop::Cx) -> bool {
    cx.fixes.autocorrect && crate::autocorrect::disable_uncorrectable()
}

/// `no_space_offenses(node, left, right, message, start_ok:, end_ok:)`; `uncorrectable` is
/// `autocorrect_with_disable_uncorrectable?`.
pub fn no_space_offenses(
    src: &Source,
    left: Option<&Token>,
    right: Option<&Token>,
    start_ok: bool,
    end_ok: bool,
    uncorrectable: bool,
) -> Vec<Planned> {
    let mut out = Vec::new();
    if extra_space(src, left, Side::Left) && !start_ok {
        out.push((*left.unwrap(), Side::Right, NO_SPACE_COMMAND));
    }
    if extra_space(src, right, Side::Right) && !end_ok && !(uncorrectable && !start_ok) {
        out.push((*right.unwrap(), Side::Left, NO_SPACE_COMMAND));
    }
    out
}

/// `space_offenses(node, left, right, message, start_ok:, end_ok:)`.
pub fn space_offenses(
    src: &Source,
    left: Option<&Token>,
    right: Option<&Token>,
    start_ok: bool,
    end_ok: bool,
    uncorrectable: bool,
) -> Vec<Planned> {
    let mut out = Vec::new();
    if !(extra_space(src, left, Side::Left) || start_ok) {
        // ponytail: RuboCop would crash on a nil token here; callers never pass one without start_ok.
        if let Some(l) = left {
            out.push((*l, Side::None, SPACE_COMMAND));
        }
    }
    if !(extra_space(src, right, Side::Right) || end_ok || (uncorrectable && !start_ok))
        && let Some(r) = right
    {
        out.push((*r, Side::None, SPACE_COMMAND));
    }
    out
}

/// `offending_empty_space?` / `offending_empty_no_space?`: the command of the empty-brackets
/// offense, if any.
pub fn empty_offense_command(src: &Source, config: &str, left: &Token, right: &Token) -> Option<&'static str> {
    let space_between = left.end + 1 == right.start && src.text.as_bytes().get(left.end) == Some(&b' ');
    if config == "space" && !space_between {
        Some("Use one")
    } else if config == "no_space" && left.end != right.start {
        Some("Do not use")
    } else {
        None
    }
}

/// SpaceCorrector.empty_corrections.
pub fn empty_corrections(src: &Source, c: &mut Corrector, config: &str, left: &Token, right: &Token) {
    match empty_offense_command(src, config, left, right) {
        Some("Use one") => {
            c.remove(left.end, right.start);
            c.insert_after(left.start, left.end, " ");
        }
        Some(_) => c.remove(left.end, right.start),
        None => {}
    }
}

/// SpaceCorrector.remove_space.
pub fn remove_space(src: &Source, c: &mut Corrector, left: &Token, right: &Token) {
    if space_after(src, left) {
        let (s, e) = side_space_range(src, left.start, left.end, Side::Right, false);
        c.remove(s, e);
    }
    if space_before(src, right) {
        let (s, e) = side_space_range(src, right.start, right.end, Side::Left, false);
        c.remove(s, e);
    }
}

/// SpaceCorrector.add_space.
pub fn add_space(src: &Source, c: &mut Corrector, left: &Token, right: &Token) {
    if !space_after(src, left) {
        c.insert_after(left.start, left.end, " ");
    }
    if !space_before(src, right) {
        c.insert_before(right.start, right.end, " ");
    }
}
