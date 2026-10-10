//! Parser-gem shapes of `if` nodes that Prism represents differently.

use ruby_prism::Node;

/// End of parser's `if` range: `end`, or for an `elsif` (no `end` in parser) its last part
/// (Parser::Builders::Default#condition_map: else body, else keyword, body, `then`, condition).
pub fn parser_end(node: &Node) -> usize {
    let Some(n) = node.as_if_node() else { return node.location().end_offset() };
    if n.if_keyword_loc().is_some_and(|k| k.as_slice() != b"elsif") {
        return node.location().end_offset();
    }
    match n.subsequent() {
        Some(s) => match s.as_else_node() {
            Some(e) => e.statements().map_or(e.else_keyword_loc().end_offset(), |s| s.location().end_offset()),
            None => parser_end(&s),
        },
        None => match (n.statements(), n.then_keyword_loc()) {
            (Some(s), _) => s.location().end_offset(),
            (None, Some(t)) => t.end_offset(),
            (None, None) => {
                // A `;` terminating the condition is parser's `then` token (loc.begin).
                let end = n.predicate().location().end_offset();
                let rest = &node.location().as_slice()[end - node.location().start_offset()..];
                let blanks = rest.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
                if rest.get(blanks) == Some(&b';') { end + blanks + 1 } else { end }
            }
        },
    }
}
