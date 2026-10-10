use crate::helpers::ruby_string::{float_to_s, regexp_escape};
use crate::prelude::*;

pub const NAME: &str = "Lint/ArrayLiteralInRegexp";
pub static COP: ArrayLiteralInRegexp = ArrayLiteralInRegexp;
pub struct ArrayLiteralInRegexp;

/// `value.respond_to?(:value) ? value.value : value.source` for a `LITERAL_TYPES` element, as
/// `to_s`; `None` for any other element.
fn literal_value(node: &Node) -> Option<String> {
    Some(match kind::of(node) {
        kind::STRING_NODE if !is_parser_dstr(node) => {
            String::from_utf8_lossy(node.as_string_node().unwrap().unescaped()).into_owned()
        }
        kind::SYMBOL_NODE => String::from_utf8_lossy(node.as_symbol_node().unwrap().unescaped()).into_owned(),
        kind::INTEGER_NODE => int_literal_value(node)?.to_string(),
        kind::FLOAT_NODE => float_to_s(node.as_float_node().unwrap().value()),
        kind::TRUE_NODE | kind::FALSE_NODE | kind::NIL_NODE => node_text(node).to_string(),
        _ => return None,
    })
}

impl Cop for ArrayLiteralInRegexp {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INTERPOLATED_REGULAR_EXPRESSION_NODE, kind::INTERPOLATED_MATCH_LAST_LINE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let parts: Vec<Node> = match node.as_interpolated_regular_expression_node() {
            Some(r) => r.parts().iter().collect(),
            None => node.as_interpolated_match_last_line_node().unwrap().parts().iter().collect(),
        };
        for part in &parts {
            let Some(embedded) = part.as_embedded_statements_node() else { continue };
            let Some(last) = embedded.statements().and_then(|s| s.body().iter().last()) else { continue };
            let Some(array) = last.as_array_node() else { continue };
            let loc = part.location();
            let (start, end) = (loc.start_offset(), loc.end_offset());
            let values: Option<Vec<String>> = array.elements().iter().map(|e| literal_value(&e)).collect();
            let Some(values) = values else {
                cx.add_with(
                    start,
                    end,
                    "Use alternation or a character class instead of interpolating an array in a regexp.",
                    false,
                );
                continue;
            };
            let escaped: Vec<String> = values.iter().map(|v| regexp_escape(v)).collect();
            let (msg, replacement) = if values.iter().all(|v| v.chars().count() == 1) {
                ("Use a character class instead of interpolating an array in a regexp.", format!("[{}]", escaped.join("")))
            } else {
                ("Use alternation instead of interpolating an array in a regexp.", format!("(?:{})", escaped.join("|")))
            };
            cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
        }
    }
}
