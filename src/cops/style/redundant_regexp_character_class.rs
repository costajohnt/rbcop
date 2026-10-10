use crate::prelude::*;
use helpers::regexp_parser::{MemberKind, literal, parse};

pub const NAME: &str = "Style/RedundantRegexpCharacterClass";
pub static COP: RedundantRegexpCharacterClass = RedundantRegexpCharacterClass;
pub struct RedundantRegexpCharacterClass;

const REQUIRES_ESCAPE_OUTSIDE_CHAR_CLASS_CHARS: &[&str] = &[".", "*", "+", "?", "{", "}", "(", ")", "|", "$"];

impl Cop for RedundantRegexpCharacterClass {
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
        for set in &parsed.sets {
            let [m] = set.members.as_slice() else { continue };
            if set.negative
                || matches!(m.kind, MemberKind::Set | MemberKind::PosixClass | MemberKind::NonPosixClass)
                || m.multiple_codepoints
            {
                continue;
            }
            let elem = &lit.pattern[m.start..m.end];
            let whitespace_in_free_space = lit.extended && elem.bytes().any(|b| matches!(b, b' ' | b'\t'..=b'\r'));
            let backreference = elem.len() == 2 && elem.starts_with('\\') && matches!(elem.as_bytes()[1], b'1'..=b'9');
            if whitespace_in_free_space
                || elem == "\\b"
                || backreference
                || REQUIRES_ESCAPE_OUTSIDE_CHAR_CLASS_CHARS.contains(&elem)
            {
                continue;
            }
            // loc.body: the set without its quantifier, located by character offsets.
            let start = lit.source_offset(src, set.start);
            let len = lit.char_index(set.end) - lit.char_index(set.start);
            let end = helpers::regexp_parser::advance_chars(src, start, len);
            let class = &src[start..end];
            let inner: String = {
                let chars: Vec<char> = class.chars().collect();
                if chars.len() >= 2 { chars[1..chars.len() - 1].iter().collect() } else { String::new() }
            };
            let element = if class == "[#]" { format!("\\{inner}") } else { inner };
            let msg = format!("Redundant single-element character class, `{class}` can be replaced with `{element}`.");
            cx.add_fix(start, end, msg, |c| c.replace(start, end, &element));
        }
    }
}
