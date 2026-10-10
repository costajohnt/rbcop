//! RuboCop's IntegerNode mixin, plus the parser-gem range of a numeric literal.

use ruby_prism::Node;

/// IntegerNode#integer_part: the source without a leading sign, up to `e`/`E`/`.`.
pub fn integer_part(source: &str) -> &str {
    let s = source.strip_prefix(['+', '-']).unwrap_or(source);
    s.split(['e', 'E', '.']).next().unwrap_or("")
}

/// The parser `int`/`float` node range for a Prism Integer/Float node, or None where parser has
/// no such node (the numeric part of an imaginary literal). Parser folds `- 12345` (a `-@` call on
/// a literal, with whitespace) into one negative literal.
pub fn literal_range(node: &Node, ancestors: &[Node]) -> Option<(usize, usize)> {
    let loc = node.location();
    match ancestors.last() {
        Some(p) if p.as_imaginary_node().is_some() => None,
        Some(p) => match p.as_call_node() {
            Some(c)
                if c.name().as_slice() == b"-@"
                    && c.call_operator_loc().is_none()
                    && c.receiver().is_some_and(|r| r.location().start_offset() == loc.start_offset())
                    && c.message_loc().is_some_and(|m| m.end_offset() == m.start_offset() + 1)
                    && c.location().start_offset() + 1 < loc.start_offset() =>
            {
                Some((c.location().start_offset(), loc.end_offset()))
            }
            _ => Some((loc.start_offset(), loc.end_offset())),
        },
        None => Some((loc.start_offset(), loc.end_offset())),
    }
}
