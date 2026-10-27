use crate::helpers::parser_tokens::{space_after, space_before, translated_sorted, within};
use crate::helpers::surrounding_space::{self as ss, Planned, Side};
use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideArrayLiteralBrackets";
pub static COP: SpaceInsideArrayLiteralBrackets = SpaceInsideArrayLiteralBrackets;
pub struct SpaceInsideArrayLiteralBrackets;

const MSG: &str = "%<command>s space inside array brackets.";
const EMPTY_MSG: &str = "%<command>s space inside empty array brackets.";

#[derive(Default)]
struct State {
    tokens: Option<Vec<Token>>,
    ignored: Vec<(usize, usize)>,
}

fn left_bracket(t: &Token) -> bool {
    matches!(t.kind, sys::PM_TOKEN_BRACKET_LEFT_ARRAY | sys::PM_TOKEN_BRACKET_LEFT)
}

fn right_bracket(t: &Token) -> bool {
    t.kind == sys::PM_TOKEN_BRACKET_RIGHT
}

fn pattern_constant(node: &Node) -> bool {
    node.as_array_pattern_node().is_some_and(|n| n.constant().is_some())
        || node.as_hash_pattern_node().is_some_and(|n| n.constant().is_some())
        || node.as_find_pattern_node().is_some_and(|n| n.constant().is_some())
}

/// `multi_dimensional_array?(tokens, token, side:)` with `token` at index `i`.
fn multi_dimensional(tokens: &[Token], i: usize, left: bool) -> bool {
    let mut j = i as isize + if left { 1 } else { -1 };
    let at = |j: isize| tokens.get(if j < 0 { (tokens.len() as isize + j) as usize } else { j as usize });
    while at(j).is_some_and(|t| t.kind == sys::PM_TOKEN_NEWLINE) {
        j += if left { 1 } else { -1 };
    }
    at(j).is_some_and(|t| if left { left_bracket(t) } else { right_bracket(t) })
}

impl Cop for SpaceInsideArrayLiteralBrackets {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE, kind::ARRAY_PATTERN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(a) = node.as_array_node()
            && !a.opening_loc().is_some_and(|l| l.as_slice() == b"[")
        {
            return;
        }
        // find_node_with_brackets: the nearest const_pattern ancestor (a Prism pattern with a constant).
        // `Const[...]` is one Prism node standing for both.
        let loc = if pattern_constant(node) {
            node.location()
        } else {
            cx.ancestors.iter().rev().find(|a| pattern_constant(a)).map_or_else(|| node.location(), |a| a.location())
        };
        let node_range = (loc.start_offset(), loc.end_offset());
        let src = cx.src;
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("no_space").to_string();
        let empty_config = cx.cfg.str("EnforcedStyleForEmptyBrackets").unwrap_or("no_space").to_string();
        let mut st = std::mem::take(cx.state::<State>());
        let all = st.tokens.get_or_insert_with(|| translated_sorted(src));
        let tokens = within(all, node_range.0, node_range.1).to_vec();
        let li = tokens.iter().position(left_bracket);
        let ri = tokens.iter().rposition(right_bracket);
        let (Some(li), Some(ri)) = (li, ri) else {
            *cx.state::<State>() = st;
            return;
        };
        let (left, right) = (tokens[li], tokens[ri]);
        let line = |t: &Token| src.line_of(t.start);
        let end_has_own_line = {
            let col = src.column(right.start);
            col == 0 || src.lines[line(&right) - 1].chars().take(col).all(helpers::ruby_string::is_space)
        };
        // closing_bracket_kept_apart?: `]` on its own line, no `[[`, and `[` not on the line of
        // `node.children.first`.
        let first_child = match node.as_array_node() {
            Some(a) => a.elements().iter().next(),
            None => {
                let p = node.as_array_pattern_node().unwrap();
                p.requireds().iter().chain(p.rest()).chain(p.posts().iter()).min_by_key(|n| n.location().start_offset())
            }
        };
        let kept_apart = end_has_own_line
            && !multi_dimensional(&tokens, li, true)
            && first_child.is_none_or(|f| src.line_of(f.location().start_offset()) != line(&left));

        // The node's full correction (`autocorrect(corrector, node)`).
        let correct = |c: &mut Corrector| {
            if li + 1 == ri {
                ss::empty_corrections(src, c, &empty_config, &left, &right);
            } else if style == "no_space" {
                ss::remove_space(src, c, &left, &right);
            } else if style == "space" {
                ss::add_space(src, c, &left, &right);
            } else {
                if multi_dimensional(&tokens, li, true) {
                    let (s, e) = ss::side_space_range(src, left.start, left.end, Side::Right, true);
                    c.remove(s, e);
                } else if !space_after(src, &left) {
                    c.insert_after(left.start, left.end, " ");
                }
                if kept_apart {
                    return;
                }
                if multi_dimensional(&tokens, ri, false) {
                    let (s, e) = ss::side_space_range(src, right.start, right.end, Side::Left, true);
                    c.remove(s, e);
                } else if !space_before(src, &right) {
                    c.insert_before(right.start, right.end, " ");
                }
            }
        };

        if li + 1 == ri {
            if let Some(command) = ss::empty_offense_command(src, &empty_config, &left, &right) {
                cx.add_fix(left.start, right.end, EMPTY_MSG.replace("%<command>s", command), correct);
            }
            *cx.state::<State>() = st;
            return;
        }

        let start_ok = line(&tokens[li + 1]) != line(&left);
        let end_ok = src.line_of(node_range.0) != src.line_of(node_range.1) && end_has_own_line;

        let unc = ss::uncorrectable(cx);
        let planned: Vec<Planned> = match style.as_str() {
            "no_space" => {
                let start_ok = tokens[li + 1].kind == sys::PM_TOKEN_COMMENT;
                ss::no_space_offenses(src, Some(&left), Some(&right), start_ok, end_ok, unc)
            }
            "space" => ss::space_offenses(src, Some(&left), Some(&right), start_ok, end_ok, unc),
            _ => {
                let mut out = Vec::new();
                let md_left = multi_dimensional(&tokens, li, true);
                if md_left && space_after(src, &left) {
                    out.push((left, Side::Right, ss::NO_SPACE_COMMAND));
                } else if !md_left {
                    out.extend(ss::space_offenses(src, Some(&left), None, start_ok, true, unc));
                }
                let md_right = multi_dimensional(&tokens, ri, false);
                if !kept_apart {
                    if md_right && space_before(src, &right) {
                        out.push((right, Side::Left, ss::NO_SPACE_COMMAND));
                    } else if !md_right {
                        out.extend(ss::space_offenses(src, None, Some(&right), true, end_ok, unc));
                    }
                }
                out
            }
        };
        for p in planned {
            ss::space_offense(cx, &mut st.ignored, node_range, p, MSG, &correct);
        }
        *cx.state::<State>() = st;
    }
}
