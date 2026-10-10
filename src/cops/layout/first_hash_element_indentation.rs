use crate::prelude::*;
use helpers::alignment::{corrector_edits, enabled_cop_str, indentation_width};
use helpers::multiline_element_indentation::{Words, check_right, claiming_paren, first_message, indent_base};

pub const NAME: &str = "Layout/FirstHashElementIndentation";
pub static COP: FirstHashElementIndentation = FirstHashElementIndentation;
pub struct FirstHashElementIndentation;

const WORDS: Words = Words {
    brace_style: "align_braces",
    what: "hash",
    bracket: "brace",
    opening: "opening brace",
    left_name: "left curly brace",
};

/// Parser key source length: a `key:` label's symbol excludes the colon.
fn key_length(key: &Node) -> usize {
    let loc = key.location();
    let mut text = node_text(key);
    let label = match kind::of(key) {
        kind::SYMBOL_NODE => key.as_symbol_node().unwrap().closing_loc().is_some_and(|c| c.as_slice() == b":"),
        kind::INTERPOLATED_SYMBOL_NODE => {
            key.as_interpolated_symbol_node().unwrap().closing_loc().is_some_and(|c| c.as_slice().ends_with(b":"))
        }
        _ => false,
    };
    if label && text.ends_with(':') && loc.end_offset() > loc.start_offset() {
        text = &text[..text.len() - 1];
    }
    text.chars().count()
}

impl Cop for FirstHashElementIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let hash = node.as_hash_node().unwrap();
        let (open, close) = (hash.opening_loc(), hash.closing_loc());
        let src = cx.src;
        let left = open.start_offset();
        let fixed_args = enabled_cop_str(cx, "Layout/ArgumentAlignment", "EnforcedStyle").as_deref()
            == Some("with_fixed_indentation");
        let paren = if fixed_args { None } else { claiming_paren(src, node, cx.ancestors, left) };
        let n = cx.ancestors.len();
        let parent = n.checked_sub(1).map(|i| &cx.ancestors[i]);
        let grandparent = n.checked_sub(2).map(|i| &cx.ancestors[i]);
        let pairs: Vec<_> = hash.elements().iter().filter_map(|e| e.as_assoc_node()).collect();
        let base = |cx: &Cx| indent_base(cx, WORDS.brace_style, left, !pairs.is_empty(), paren, parent, grandparent);

        if let Some(first) = pairs.first() {
            let (start, end) = (first.location().start_offset(), first.location().end_offset());
            if src.line_of(start) == src.line_of(left) {
                return;
            }
            let rocket = first.operator_loc().is_some_and(|o| o.as_slice() == b"=>");
            let key = if rocket { "EnforcedHashRocketStyle" } else { "EnforcedColonStyle" };
            let separator = cx.config.for_cop("Layout/HashAlignment").str(key) == Some("separator");
            let offset = if separator {
                let lengths: Vec<usize> = pairs.iter().map(|p| key_length(&p.key())).collect();
                (lengths.iter().max().unwrap() - lengths[0]) as i64
            } else {
                0
            };
            let (b, t) = base(cx);
            let delta = b + indentation_width(cx) + offset - src.column(start) as i64;
            if delta != 0 {
                let key_line = src.line_of(first.key().location().start_offset());
                let fixable = if src.line_of(first.value().location().start_offset()) <= key_line {
                    corrector_edits(src, start, end, delta, Some(&first.as_node()))
                } else {
                    let ls = src.line_start(key_line);
                    let le = ls + src.lines.get(key_line - 1).map_or(0, |l| l.len());
                    corrector_edits(src, ls, le, delta, None)
                };
                cx.add_with(start, end, first_message(cx, &WORDS, &t), fixable);
            }
        }
        check_right(cx, &WORDS, (close.start_offset(), close.end_offset()), base(cx));
    }
}
