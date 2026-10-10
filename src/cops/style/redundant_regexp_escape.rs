use crate::prelude::*;
use helpers::regexp_parser::{advance_chars, literal, parse};

pub const NAME: &str = "Style/RedundantRegexpEscape";
pub static COP: RedundantRegexpEscape = RedundantRegexpEscape;
pub struct RedundantRegexpEscape;

const ALLOWED_ALWAYS_ESCAPES: &str = " \n[]^\\#";
const ALLOWED_OUTSIDE_CHAR_CLASS_METACHAR_ESCAPES: &str = ".*+?{}()|$";

impl Cop for RedundantRegexpEscape {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::REGULAR_EXPRESSION_NODE,
            kind::INTERPOLATED_REGULAR_EXPRESSION_NODE,
            kind::MATCH_LAST_LINE_NODE,
            kind::INTERPOLATED_MATCH_LAST_LINE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(lit) = literal(node) else { return };
        let Some(parsed) = parse(&lit.pattern, lit.extended) else { return };
        let src = cx.src.text;
        let open_last = src[lit.open.0..lit.open.1].chars().last();
        let close_first = src[lit.close.0..lit.close.1].chars().next();
        for e in &parsed.escapes {
            let Some(ch) = lit.pattern[e.start + 1..].chars().next() else { continue };
            let index = lit.char_index(e.start) as isize;
            let allowed = ch.is_alphanumeric()
                || ALLOWED_ALWAYS_ESCAPES.contains(ch)
                || Some(ch) == open_last
                || Some(ch) == close_first
                || (index > 0 && (ch == '@' || ch == '$') && lit.content_char(src, index - 1) == Some('#'))
                || if e.in_set {
                    ch == '-' && !begins_or_ends_with_escaped_hyphen(&lit, src, index)
                } else {
                    ALLOWED_OUTSIDE_CHAR_CLASS_METACHAR_ESCAPES.contains(ch)
                };
            if allowed {
                continue;
            }
            let start = lit.source_offset(src, e.start);
            let end = advance_chars(src, start, 2);
            let one = advance_chars(src, start, 1);
            cx.add_fix(start, end, "Redundant escape inside regexp literal", |c| c.remove(start, one));
        }
    }
}

/// char_class_begins_or_ends_with_escaped_hyphen?
fn begins_or_ends_with_escaped_hyphen(lit: &helpers::regexp_parser::Literal, src: &str, index: isize) -> bool {
    if lit.content_char(src, index + 2) == Some(']') {
        true
    } else if lit.content_char(src, index - 1) == Some('[') {
        index < 2 || lit.content_char(src, index - 2) != Some('\\')
    } else {
        false
    }
}
