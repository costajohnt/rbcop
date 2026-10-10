use crate::prelude::*;
use helpers::alignment::{corrector_edits, enabled_cop_str, indentation_width};
use helpers::multiline_element_indentation::{IndentBase, Words, check_right, claiming_paren, first_message, indent_base};

pub const NAME: &str = "Layout/FirstArrayElementIndentation";
pub static COP: FirstArrayElementIndentation = FirstArrayElementIndentation;
pub struct FirstArrayElementIndentation;

const WORDS: Words = Words {
    brace_style: "align_brackets",
    what: "array",
    bracket: "bracket",
    opening: "opening bracket",
    left_name: "left square bracket",
};

impl Cop for FirstArrayElementIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        let (Some(open), Some(close)) = (array.opening_loc(), array.closing_loc()) else { return };
        let src = cx.src;
        let left = open.start_offset();
        let paren = claiming_paren(src, node, cx.ancestors, left);
        let n = cx.ancestors.len();
        let parent = n.checked_sub(1).map(|i| &cx.ancestors[i]);
        let grandparent = n.checked_sub(2).map(|i| &cx.ancestors[i]);
        let first = array.elements().iter().next();
        let base = |cx: &Cx| indent_base(cx, WORDS.brace_style, left, first.is_some(), paren, parent, grandparent);

        // autocorrect_incompatible_with_other_cops?
        if enabled_cop_str(cx, "Layout/ArrayAlignment", "EnforcedStyle").as_deref() == Some("with_fixed_indentation")
            && (cx.cfg.str("EnforcedStyle") != Some("consistent")
                || (array.elements().iter().count() >= 2 && !matches!(base(cx).1, IndentBase::StartOfLine)))
        {
            return;
        }

        if let Some(first) = &first {
            let start = first.location().start_offset();
            if src.line_of(start) == src.line_of(left) {
                return;
            }
            let (b, t) = base(cx);
            let delta = b + indentation_width(cx) - src.column(start) as i64;
            if delta != 0 {
                let end = first.location().end_offset();
                let fixable = corrector_edits(src, start, end, delta, Some(first));
                cx.add_with(start, end, first_message(cx, &WORDS, &t), fixable);
            }
        }
        check_right(cx, &WORDS, (close.start_offset(), close.end_offset()), base(cx));
    }
}
