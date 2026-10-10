use crate::helpers::percent_literal::percent_type;
use crate::prelude::*;

pub const NAME: &str = "Lint/NestedPercentLiteral";
pub static COP: NestedPercentLiteral = NestedPercentLiteral;
pub struct NestedPercentLiteral;

/// PreferredDelimiters::PERCENT_LITERAL_TYPES
const TYPES: [&str; 10] = ["%", "%i", "%I", "%q", "%Q", "%r", "%s", "%w", "%W", "%x"];

/// `/\A#{type}\W/` for any type (Ruby's `\W` is ASCII-only).
fn nested(literal: &str) -> bool {
    TYPES.iter().any(|t| {
        literal
            .strip_prefix(t)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| !(c.is_ascii_alphanumeric() || c == '_'))
    })
}

impl Cop for NestedPercentLiteral {
    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        let Some(t) = percent_type(&array) else { return };
        if !TYPES.contains(&t) {
            return;
        }
        // `child.children.first.to_s`: only plain str/sym elements can match (an interpolated
        // element's first child prints as an s-expression).
        let hit = array.elements().iter().any(|e| {
            let value = match (e.as_string_node(), e.as_symbol_node()) {
                (Some(s), _) => s.unescaped().to_vec(),
                (_, Some(s)) => s.unescaped().to_vec(),
                _ => return false,
            };
            nested(&String::from_utf8_lossy(&value))
        });
        if hit {
            let l = node.location();
            cx.add(
                l.start_offset(),
                l.end_offset(),
                "Within percent literals, nested percent literals do not function and may be unwanted in the result.",
            );
        }
    }
}
