//! RuboCop's PercentLiteral mixin.

use ruby_prism::ArrayNode;

/// `type(node)` when `percent_literal?(node)`: the opening delimiter without its last character
/// (`%w[` -> `%w`); None when the array does not start with `%`.
pub fn percent_type<'pr>(node: &ArrayNode<'pr>) -> Option<&'pr str> {
    let open = std::str::from_utf8(node.opening_loc()?.as_slice()).ok()?;
    let mut chars = open.chars();
    chars.next_back();
    open.starts_with('%').then_some(chars.as_str())
}
