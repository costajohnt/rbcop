use crate::prelude::*;

pub const NAME: &str = "Lint/DuplicateMatchPattern";
pub static COP: DuplicateMatchPattern = DuplicateMatchPattern;
pub struct DuplicateMatchPattern;

/// `pattern.children.map(&:source).sort.to_s` for hash_pattern / match_alt, else `pattern.source`.
fn pattern_source(pattern: &Node) -> String {
    let children: Option<Vec<Node>> = if let Some(h) = pattern.as_hash_pattern_node().filter(|h| h.constant().is_none()) {
        Some(h.elements().iter().chain(h.rest()).collect())
    } else {
        pattern.as_alternation_pattern_node().map(|a| vec![a.left(), a.right()])
    };
    let Some(children) = children else { return node_text(pattern).to_string() };
    let mut sources: Vec<&[u8]> = children.iter().map(|c| c.location().as_slice()).collect();
    sources.sort();
    let items: Vec<String> = sources.iter().map(|s| helpers::ruby_string::inspect(s)).collect();
    format!("[{}]", items.join(", "))
}

impl Cop for DuplicateMatchPattern {
    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_MATCH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let mut previous: Vec<String> = Vec::new();
        for in_node in node.as_case_match_node().unwrap().conditions().iter() {
            let pattern = in_node.as_in_node().unwrap().pattern();
            // Prism wraps a guarded pattern in an IfNode/UnlessNode; parser has (in_pattern pat guard body).
            let (pattern, guard) = match (pattern.as_if_node(), pattern.as_unless_node()) {
                (Some(i), _) => (
                    i.statements().and_then(|s| s.body().iter().next()),
                    Some(cx.src.text[i.if_keyword_loc().unwrap().start_offset()..i.predicate().location().end_offset()].to_string()),
                ),
                (_, Some(u)) => (
                    u.statements().and_then(|s| s.body().iter().next()),
                    Some(cx.src.text[u.keyword_loc().start_offset()..u.predicate().location().end_offset()].to_string()),
                ),
                _ => (Some(pattern), None),
            };
            let Some(pattern) = pattern else { continue };
            let identity = pattern_source(&pattern) + guard.as_deref().unwrap_or("");
            if previous.contains(&identity) {
                let l = pattern.location();
                cx.add(l.start_offset(), l.end_offset(), "Duplicate `in` pattern detected.");
            } else {
                previous.push(identity);
            }
        }
    }
}
