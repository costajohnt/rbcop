//! Parser-gem `resbody` views of Prism's RescueNode / RescueModifierNode.

use crate::source::Source;
use ruby_prism::{Node, RescueNode};

/// Byte range of parser's `resbody` for a `rescue` clause. Prism's RescueNode also spans the
/// following clauses; parser's ends at the body, else at the last of keyword / exceptions /
/// `=> var`, plus a trailing `;` (but not `then`) when there is no body.
pub fn range(src: &Source, node: &RescueNode) -> (usize, usize) {
    let start = node.keyword_loc().start_offset();
    if let Some(s) = node.statements() {
        return (start, s.location().end_offset());
    }
    let end = [
        Some(node.keyword_loc().end_offset()),
        node.exceptions().iter().last().map(|e| e.location().end_offset()),
        node.reference().map(|r| r.location().end_offset()),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap();
    (start, semicolon_end(end, &src.text.as_bytes()[end..]))
}

/// The translation layer's `srange_semicolon`: a `;` after the clause head (`rest` is the source
/// from `end` on), past Ruby whitespace (newlines too), ends the bodiless `resbody` there.
pub fn semicolon_end(end: usize, rest: &[u8]) -> usize {
    let ws = rest.iter().take_while(|&&b| crate::helpers::parser_tokens::is_ruby_space(b)).count();
    if rest.get(ws) == Some(&b';') { end + ws + 1 } else { end }
}

/// The `resbody` range of a modifier rescue (`x rescue y`): keyword to the rescue expression.
pub fn modifier_range(node: &ruby_prism::RescueModifierNode) -> (usize, usize) {
    (node.keyword_loc().start_offset(), node.rescue_expression().location().end_offset())
}

/// The nodes RuboCop's `on_resbody` handles: RescueNode and RescueModifierNode.
pub fn exceptions<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    node.as_rescue_node().map(|r| r.exceptions().iter().collect()).unwrap_or_default()
}
