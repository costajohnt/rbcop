//! RuboCop's FrozenStringLiteral mixin (magic-comment queries over the leading comment lines).

use crate::config::value::Value;
use crate::cop::Cx;
use crate::cops::style_frozen_string_literal_comment::frozen_value;

/// `leading_comment_lines`: the lines before the first code line.
fn leading<'a>(cx: &Cx<'a, '_>) -> &'a [&'a str] {
    let src = cx.src;
    match cx.facts.first_code {
        Some(off) => &src.lines[..(src.line_of(off) - 1).min(src.lines.len())],
        None => &src.lines[..],
    }
}

/// `string_literals_frozen_by_default?` (AllCops: true, false or nil).
pub fn frozen_by_default(cx: &Cx) -> Option<bool> {
    match cx.config.all_cops().get("StringLiteralsFrozenByDefault") {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

/// `frozen_string_literals_enabled?`.
pub fn enabled(cx: &Cx) -> bool {
    match leading(cx).iter().find_map(|l| frozen_value(l)) {
        Some(v) => v == "true",
        None => frozen_by_default(cx) == Some(true),
    }
}

/// `frozen_string_literals_disabled?`.
pub fn disabled(cx: &Cx) -> bool {
    leading(cx).iter().any(|l| frozen_value(l).as_deref() == Some("false"))
}

/// `leading_comment_lines` (the lines before the first non-comment token).
pub fn leading_comment_lines<'a>(cx: &Cx<'a, '_>) -> &'a [&'a str] {
    leading(cx)
}

/// FrozenStringLiteral#uninterpolated_string? (from Ruby 3.0; any str/dstr before): a str, or a
/// dstr without begin/ivar/cvar/gvar descendants.
fn frozen_string_type(cx: &Cx, node: &ruby_prism::Node) -> bool {
    use ruby_prism::Node;
    fn uninterpolated(parts: ruby_prism::NodeList) -> bool {
        parts.iter().all(|p| match &p {
            Node::EmbeddedStatementsNode { .. } => false,
            Node::EmbeddedVariableNode { .. } => !matches!(
                p.as_embedded_variable_node().unwrap().variable(),
                Node::InstanceVariableReadNode { .. } | Node::ClassVariableReadNode { .. } | Node::GlobalVariableReadNode { .. }
            ),
            Node::InterpolatedStringNode { .. } => uninterpolated(p.as_interpolated_string_node().unwrap().parts()),
            _ => true,
        })
    }
    match node {
        Node::StringNode { .. } | Node::SourceFileNode { .. } => true,
        Node::InterpolatedStringNode { .. } => {
            cx.config.target_ruby < 3.0 || uninterpolated(node.as_interpolated_string_node().unwrap().parts())
        }
        _ => false,
    }
}

/// FrozenStringLiteral#frozen_string_literal?
pub fn frozen_string_literal(cx: &Cx, node: &ruby_prism::Node) -> bool {
    frozen_string_type(cx, node) && enabled(cx)
}
