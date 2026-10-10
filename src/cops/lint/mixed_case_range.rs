use crate::prelude::*;
use helpers::regexp_parser::{MemberKind, advance_chars, literal, parse};

pub const NAME: &str = "Lint/MixedCaseRange";
pub static COP: MixedCaseRange = MixedCaseRange;
pub struct MixedCaseRange;

const MSG: &str = "Ranges from upper to lower case ASCII letters may include unintended characters. \
                   Instead of `A-z` (which also includes several symbols) specify each range individually: \
                   `A-Za-z` and individually specify any symbols.";

/// `range_for(char)`: the `'a'..'z'` / `'A'..'Z'` range holding a one-character string.
fn range_for(s: &str) -> Option<(char, char)> {
    let mut chars = s.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else { return None };
    [('a', 'z'), ('A', 'Z')].into_iter().find(|&(a, z)| (a..=z).contains(&c))
}

fn unsafe_range(a: &str, b: &str) -> bool {
    a.chars().count() == 1 && b.chars().count() == 1 && range_for(a) != range_for(b)
}

/// `regexp_range(source)` for a `x-y` range.
fn regexp_range(open: char, close: char) -> Option<String> {
    let (_, open_end) = range_for(&open.to_string())?;
    let (close_begin, _) = range_for(&close.to_string())?;
    let join = |a: char, b: char| if a == b { a.to_string() } else { format!("{a}-{b}") };
    Some(format!("{}{}", join(open, open_end), join(close_begin, close)))
}

impl Cop for MixedCaseRange {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::RANGE_NODE,
            kind::REGULAR_EXPRESSION_NODE,
            kind::INTERPOLATED_REGULAR_EXPRESSION_NODE,
            kind::MATCH_LAST_LINE_NODE,
            kind::INTERPOLATED_MATCH_LAST_LINE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(r) = node.as_range_node() {
            let value = |n: Option<Node>| {
                let n = n?;
                let s = n.as_string_node().filter(|_| !is_parser_dstr(&n))?;
                Some(String::from_utf8_lossy(s.unescaped()).into_owned())
            };
            let (Some(a), Some(b)) = (value(r.left()), value(r.right())) else { return };
            if unsafe_range(&a, &b) {
                let l = node.location();
                cx.add_with(l.start_offset(), l.end_offset(), MSG, false);
            }
            return;
        }
        let Some(lit) = literal(node) else { return };
        let Some(parsed) = parse(&lit.pattern, lit.extended) else { return };
        let src = cx.src.text;
        let mut offenses = Vec::new();
        for set in &parsed.sets {
            for m in set.members.iter().filter(|m| m.kind == MemberKind::Set) {
                // A range of two literal characters (escapes, nested sets and intersections are skipped).
                let chars: Vec<char> = lit.pattern[m.start..m.end].chars().collect();
                let [open, '-', close] = chars[..] else { continue };
                if matches!(open, '\\' | '[') || matches!(close, '\\' | '[') {
                    continue;
                }
                if !unsafe_range(&open.to_string(), &close.to_string()) {
                    continue;
                }
                let Some(replacement) = regexp_range(open, close) else { continue };
                let start = lit.source_offset(src, m.start);
                let end = advance_chars(src, start, 3);
                offenses.push((start, end, replacement));
            }
        }
        for (start, end, replacement) in offenses {
            cx.add_fix(start, end, MSG, |c| c.replace(start, end, &replacement));
        }
    }
}
