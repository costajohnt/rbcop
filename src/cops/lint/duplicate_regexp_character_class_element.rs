use crate::prelude::*;
use helpers::regexp_parser::{MemberKind, advance_chars, literal, parse};

pub const NAME: &str = "Lint/DuplicateRegexpCharacterClassElement";
pub static COP: DuplicateRegexpCharacterClassElement = DuplicateRegexpCharacterClassElement;
pub struct DuplicateRegexpCharacterClassElement;

const MSG: &str = "Duplicate element inside regexp character class";

impl Cop for DuplicateRegexpCharacterClassElement {
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
        // `#{...}` source ranges (parser `begin` children).
        let parts = match (node.as_interpolated_regular_expression_node(), node.as_interpolated_match_last_line_node()) {
            (Some(r), _) => Some(r.parts()),
            (_, Some(r)) => Some(r.parts()),
            _ => None,
        };
        let interpolations: Vec<(usize, usize)> = parts
            .into_iter()
            .flat_map(|p| p.iter())
            .filter(|p| p.as_embedded_statements_node().is_some())
            .map(|p| (p.location().start_offset(), p.location().end_offset()))
            .collect();
        // `expression` of a pattern byte range, located by characters from the end of `loc.begin`.
        let source_range = |from: usize, to: usize| {
            let start = lit.source_offset(src, from);
            (start, advance_chars(src, start, lit.char_index(to) - lit.char_index(from)))
        };
        let mut offenses: Vec<(usize, usize)> = Vec::new();
        // Each `[...]` set, then the ranges inside it (type :set, token :range); intersections are skipped.
        for set in &parsed.sets {
            let mut seen: Vec<&str> = Vec::new();
            for m in &set.members {
                let (start, end) = source_range(m.start, m.end);
                if interpolations.iter().any(|&(a, b)| start < b && a < end) {
                    continue;
                }
                let text = &lit.pattern[m.start..m.end];
                if seen.contains(&text) {
                    offenses.push((start, end));
                }
                seen.push(text);
            }
            for m in set.members.iter().filter(|m| m.kind == MemberKind::Set && lit.pattern.as_bytes()[m.start] != b'[') {
                // `x-x`: the second endpoint repeats the first.
                let text = &lit.pattern[m.start..m.end];
                let k = (text.len().saturating_sub(1)) / 2;
                if text.len() % 2 == 1 && text.get(k..k + 1) == Some("-") && text.get(..k) == text.get(k + 1..) {
                    let (start, end) = source_range(m.start + k + 1, m.end);
                    if !interpolations.iter().any(|&(a, b)| start < b && a < end) {
                        offenses.push((start, end));
                    }
                }
            }
        }
        for (start, end) in offenses {
            cx.add_fix(start, end, MSG, |c| c.remove(start, end));
        }
    }
}
