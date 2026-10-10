use crate::helpers::percent_literal::percent_type;
use crate::prelude::*;

pub const NAME: &str = "Lint/PercentSymbolArray";
pub static COP: PercentSymbolArray = PercentSymbolArray;
pub struct PercentSymbolArray;

impl Cop for PercentSymbolArray {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        if !matches!(percent_type(&array), Some("%i" | "%I")) {
            return;
        }
        // `child.children.first.to_s`: an interpolated symbol's first child prints as an
        // s-expression, which never starts with `:` or ends with `,`.
        let hit = array.elements().iter().any(|e| {
            let Some(s) = e.as_symbol_node() else { return false };
            let literal = String::from_utf8_lossy(s.unescaped());
            literal.chars().any(char::is_alphanumeric) && (literal.starts_with(':') || literal.ends_with(','))
        });
        if !hit {
            return;
        }
        let ranges: Vec<(usize, usize)> =
            array.elements().iter().map(|e| (e.location().start_offset(), e.location().end_offset())).collect();
        let src = cx.src;
        let l = node.location();
        let msg = "Within `%i`/`%I`, ':' and ',' are unnecessary and may be unwanted in the resulting symbols.";
        cx.add_fix(l.start_offset(), l.end_offset(), msg, |c| {
            for &(start, end) in &ranges {
                let source = src.slice(start, end);
                if source.ends_with(',') {
                    c.remove_trailing(start, end, 1);
                }
                if source.starts_with(':') {
                    c.remove_leading(start, end, 1);
                }
            }
        });
    }
}
