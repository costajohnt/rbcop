use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideBlockBraces";
pub static COP: SpaceInsideBlockBraces = SpaceInsideBlockBraces;
pub struct SpaceInsideBlockBraces;

/// `inner.split("\n").last.count(' ')`.
fn last_space_count(inner: &str) -> usize {
    inner.split('\n').rev().find(|l| !l.is_empty()).map_or(0, |l| l.matches(' ').count())
}

struct Check<'a> {
    style: &'a str,
}

impl Check<'_> {
    /// `enforced`: the offense is about `EnforcedStyle` (not `EnforcedStyleForEmptyBraces`).
    fn offense(&self, cx: &mut Cx, s: isize, e: isize, msg: &str, enforced: bool) {
        if s > e {
            return;
        }
        let (s, e) = (s as usize, e as usize);
        if enforced && cx.block_runs(s, e) {
            cx.opposite_style_detected();
        }
        let range_src = cx.src.slice(s, e);
        cx.add_fix(s, e, msg, |c| {
            if range_src.bytes().any(helpers::parser_tokens::is_ruby_space) {
                c.remove(s, e);
            } else if range_src == "{}" {
                c.replace(s, e, "{ }");
            } else if range_src == "{|" {
                c.replace(s, e, "{ |");
            } else {
                c.insert_before(s, e, " ");
            }
        });
    }

    fn no_space(&self, cx: &mut Cx, s: usize, e: usize, msg: &str) {
        if self.style == "space" {
            self.offense(cx, s as isize, e as isize, msg, true);
        } else {
            cx.correct_style_detected();
        }
    }

    fn space(&self, cx: &mut Cx, s: isize, e: isize, msg: &str) {
        if self.style == "no_space" {
            self.offense(cx, s, e, msg, true);
        } else {
            cx.correct_style_detected();
        }
    }
}

impl Cop for SpaceInsideBlockBraces {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (open, close, body, params, node_start) = if let Some(b) = node.as_block_node() {
            // The parser block node starts with its call.
            let start = cx.parent().map_or(node.location().start_offset(), |p| p.location().start_offset());
            (b.opening_loc(), b.closing_loc(), b.body().is_some(), b.parameters(), start)
        } else {
            let l = node.as_lambda_node().unwrap();
            (l.opening_loc(), l.closing_loc(), l.body().is_some(), l.parameters(), node.location().start_offset())
        };
        if open.as_slice() != b"{" {
            return;
        }
        let src = cx.src;
        let (lb, le) = (open.start_offset(), open.end_offset());
        let (rb, re) = (close.start_offset(), close.end_offset());
        let single_line = src.line_of(lb) == src.line_of(rb);
        if !body && !single_line {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("space").to_string();
        let empty_style = cx.cfg.str("EnforcedStyleForEmptyBraces").unwrap_or("no_space").to_string();
        let check = Check { style: &style };
        if le == rb {
            if empty_style == "space" {
                check.offense(cx, lb as isize, re as isize, "Space missing inside empty braces.", false);
            }
            return;
        }
        let inner = src.slice(le, rb);
        if !inner.bytes().any(|b| !helpers::parser_tokens::is_ruby_space(b)) {
            if empty_style == "no_space" {
                check.offense(cx, le as isize, rb as isize, "Space inside empty braces detected.", false);
            }
            return;
        }

        // args_delimiter: `|` or `(` of explicit block parameters (block_type? only).
        let delim = params.and_then(|p| p.as_block_parameters_node()).and_then(|p| p.opening_loc());
        // Below 2.7 the parser gem lexes empty `||` as one tOROP: the args node has no `|` begin.
        let oror = |d: &ruby_prism::Location| cx.config.target_ruby < 2.7 && src.text[d.end_offset()..].starts_with('|');
        let pipe = delim
            .as_ref()
            .filter(|d| d.as_slice() == b"|" && !oror(d))
            .map(|d| (d.start_offset(), d.end_offset()));
        let before_params = cx.cfg.bool("SpaceBeforeBlockParameters").unwrap_or(true);

        // check_left_brace
        let first = inner.as_bytes()[0];
        if !helpers::parser_tokens::is_ruby_space(first) {
            if let Some((ds, de)) = pipe {
                if le == ds && before_params {
                    check.offense(cx, lb as isize, de as isize, "Space between { and | missing.", true);
                } else {
                    cx.correct_style_detected();
                }
            } else {
                // One character after the brace (it may be multibyte: `{\u3000` is an identifier).
                check.no_space(cx, le, le + inner.chars().next().map_or(1, char::len_utf8), "Space missing inside {.");
            }
        } else if let Some((ds, _)) = pipe {
            if before_params {
                cx.correct_style_detected();
            } else {
                check.offense(cx, le as isize, ds as isize, "Space between { and | detected.", true);
            }
        } else {
            let (_, e) = range_with_surrounding_space(src.text, lb, le, Surrounding { side: Side::Right, ..Default::default() });
            check.space(cx, lb as isize + 1, e as isize, "Space inside { detected.");
        }

        // check_right_brace
        let ends_non_space = inner.split('\n').any(|l| l.bytes().last().is_some_and(|b| !helpers::parser_tokens::is_ruby_space(b)));
        if single_line && ends_non_space {
            check.no_space(cx, rb, re, "Space missing inside }.");
            return;
        }
        let column = src.column(node_start) as isize;
        let right_col = src.column(rb) as isize;
        if !single_line && (column == right_col || column == last_space_count(inner) as isize) {
            return;
        }
        let (ws, _) = range_with_surrounding_space(src.text, rb, re, Surrounding { side: Side::Left, ..Default::default() });
        let mut begin = ws as isize;
        let mut end = re as isize - 1;
        if src.slice(ws, re).contains('\n') {
            begin = end - (right_col - column);
        }
        if inner.ends_with(']') {
            end -= 1;
            begin = end - (last_space_count(inner) as isize - column);
        }
        check.space(cx, begin, end, "Space inside } detected.");
    }
}
