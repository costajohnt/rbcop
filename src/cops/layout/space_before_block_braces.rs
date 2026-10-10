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
        let (space_s, _) = helpers::range_help::with_surrounding_space(cx.src, brace_s, brace_e, true, true, true);
        let used = if space_s == brace_s { "no_space" } else { "space" };

        if brace_e == close.start_offset() {
            let empty_style = cx.cfg.str("EnforcedStyleForEmptyBraces").unwrap_or(style);
            if empty_style == used {
                return;
            }
            if empty_style == "space" {
                cx.add(brace_s, brace_e, MISSING);
            } else {
                cx.add(space_s, brace_s, DETECTED);
            }
        } else if used != style {
            if used == "space" {
                cx.add(space_s, brace_s, DETECTED);
            } else {
                cx.add(brace_s, brace_e, MISSING);
            }
        }
    }
}
