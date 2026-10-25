use crate::helpers::range_help::{Surrounding, range_with_surrounding_space};
use crate::autogen::Val;
use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceBeforeBlockBraces";
pub static COP: SpaceBeforeBlockBraces = SpaceBeforeBlockBraces;
pub struct SpaceBeforeBlockBraces;

const MISSING: &str = "Space missing to the left of {.";
const DETECTED: &str = "Space detected to the left of {.";

impl Cop for SpaceBeforeBlockBraces {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (open, close) = match node.as_block_node() {
            Some(b) => (b.opening_loc(), b.closing_loc()),
            None => {
                let l = node.as_lambda_node().unwrap();
                (l.opening_loc(), l.closing_loc())
            }
        };
        if open.as_slice() != b"{" {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("space");
        if style == "no_space"
            && cx.config.for_cop("Style/BlockDelimiters").str("EnforcedStyle") == Some("line_count_based")
        {
            // BlockNode#multiline?: the braces are on different lines.
            if cx.src.line_of(open.start_offset()) != cx.src.line_of(close.start_offset()) {
                return;
            }
        }
        let (brace_s, brace_e) = (open.start_offset(), open.end_offset());
        let (space_s, _) = range_with_surrounding_space(cx.src.text, brace_s, brace_e, Surrounding::default());
        let used = if space_s == brace_s { "no_space" } else { "space" };

        if brace_e == close.start_offset() {
            let empty_style = cx.cfg.str("EnforcedStyleForEmptyBraces").unwrap_or(style);
            let used_style = Val::Str(used.to_string());
            if empty_style == used {
                // handle_different_styles_for_empty_braces
                return cx.autogen(move |c| {
                    if c.get("EnforcedStyleForEmptyBraces").is_some_and(|v| *v != used_style) {
                        c.cfg = vec![("Enabled".into(), Val::Bool(false))];
                    }
                });
            }
            cx.autogen(move |c| {
                if c.get("Enabled").is_none() {
                    c.set("EnforcedStyleForEmptyBraces", used_style);
                }
            });
            if empty_style == "space" {
                cx.add_fix(brace_s, brace_e, MISSING, |c| c.insert_before(brace_s, brace_e, " "));
            } else {
                cx.add_fix(space_s, brace_s, DETECTED, |c| c.remove(space_s, brace_s));
            }
        } else if used == style {
            cx.correct_style_detected();
        } else {
            let range = if used == "space" { (space_s, brace_s) } else { (brace_s, brace_e) };
            if cx.block_runs(range.0, range.1) {
                cx.opposite_style_detected();
            }
            if used == "space" {
                cx.add_fix(space_s, brace_s, DETECTED, |c| c.remove(space_s, brace_s));
            } else {
                cx.add_fix(brace_s, brace_e, MISSING, |c| c.insert_before(brace_s, brace_e, " "));
            }
        }
    }
}
