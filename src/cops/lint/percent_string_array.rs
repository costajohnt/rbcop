use crate::helpers::percent_literal::percent_type;
use crate::prelude::*;

pub const NAME: &str = "Lint/PercentStringArray";
pub static COP: PercentStringArray = PercentStringArray;
pub struct PercentStringArray;

// Ruby's `^`/`$` are line anchors.
static QUOTES_AND_COMMAS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [Regex::new(r"(?m),$").unwrap(), Regex::new(r"(?m)^'.*'$").unwrap(), Regex::new(r#"(?m)^".*"$"#).unwrap()]
});
static LEADING_QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^['"]"#).unwrap());
static TRAILING_QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)['"]?,?$"#).unwrap());

impl Cop for PercentStringArray {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        if !matches!(percent_type(&array), Some("%w" | "%W")) {
            return;
        }
        // `value.children.first.to_s`: an interpolated value's first child prints as an
        // s-expression, which never matches.
        let hit = array.elements().iter().any(|e| {
            let Some(s) = e.as_string_node() else { return false };
            let literal = String::from_utf8_lossy(s.unescaped()).into_owned();
            literal.chars().any(char::is_alphanumeric) && QUOTES_AND_COMMAS.iter().any(|p| p.is_match(&literal))
        });
        if !hit {
            return;
        }
        let ranges: Vec<(usize, usize)> =
            array.elements().iter().map(|e| (e.location().start_offset(), e.location().end_offset())).collect();
        let src = cx.src;
        let l = node.location();
        let msg = "Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.";
        cx.add_fix(l.start_offset(), l.end_offset(), msg, |c| {
            for &(start, end) in &ranges {
                let source = src.slice(start, end);
                if let Some(m) = TRAILING_QUOTE.find(source) {
                    c.remove_trailing(start, end, m.len());
                }
                if LEADING_QUOTE.is_match(source) {
                    c.remove_leading(start, end, 1);
                }
            }
        });
    }
}
