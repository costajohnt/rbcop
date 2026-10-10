use crate::prelude::*;
use helpers::alignment::{begins_its_line, corrector_edits, display_column, first_non_space, indentation_width};

pub const NAME: &str = "Layout/ArrayAlignment";
pub static COP: ArrayAlignment = ArrayAlignment;
pub struct ArrayAlignment;

impl Cop for ArrayAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        // parser wraps `rescue A, B` exception lists in an array
        &[kind::ARRAY_NODE, kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        // (elements, line used by with_fixed_indentation)
        let (items, lineno): (Vec<Node>, usize) = if let Some(a) = node.as_array_node() {
            if cx.parent().is_some_and(|p| kind::of(p) == kind::MULTI_WRITE_NODE) {
                return;
            }
            let line = match (a.opening_loc(), cx.parent()) {
                (Some(_), _) | (None, None) => src.line_of(node.location().start_offset()),
                (None, Some(p)) => src.line_of(parent_start(p, cx.ancestors)),
            };
            (a.elements().iter().collect(), line)
        } else {
            let r = node.as_rescue_node().unwrap();
            (r.exceptions().iter().collect(), src.line_of(r.keyword_loc().start_offset()))
        };
        if items.len() < 2 {
            return;
        }
        let fixed = cx.cfg.str("EnforcedStyle") == Some("with_fixed_indentation");
        let base = if fixed {
            let line = src.lines.get(lineno - 1).copied().unwrap_or("");
            line[..first_non_space(line).unwrap_or(0)].chars().count() as i64 + indentation_width(cx)
        } else {
            display_column(src, items[0].location().start_offset()) as i64
        };
        let msg = if fixed {
            "Use one level of indentation for elements following the first line of a multi-line array."
        } else {
            "Align the elements of an array literal if they span more than one line."
        };
        let mut prev_line = 0;
        for item in &items {
            let loc = item.location();
            let (start, end) = (loc.start_offset(), loc.end_offset());
            let line = src.line_of(start);
            if line > prev_line && begins_its_line(src, start) {
                let delta = base - display_column(src, start) as i64;
                if delta != 0 {
                    let nested = cx.out.iter().any(|o| o.cop == NAME && start >= o.start && end <= o.end);
                    let fixable = !nested && corrector_edits(src, start, end, delta, Some(item));
                    cx.add_with(start, end, msg, fixable);
                }
            }
            prev_line = line;
        }
    }
}

/// Start of the parser-gem parent of an unbracketed array (`x = 1, 2`): the nearest ancestor
/// that is not a Prism-only wrapper.
fn parent_start(parent: &Node, ancestors: &[Node]) -> usize {
    ancestors
        .iter()
        .rev()
        .find(|a| frame(a) != Frame::Transparent && kind::of(a) != kind::STATEMENTS_NODE)
        .unwrap_or(parent)
        .location()
        .start_offset()
}
