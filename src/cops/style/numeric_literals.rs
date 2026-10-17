use crate::helpers::integer_node::{integer_part, literal_range};
use crate::prelude::*;

pub const NAME: &str = "Style/NumericLiterals";
pub static COP: NumericLiterals = NumericLiterals;
pub struct NumericLiterals;

const MSG: &str = "Use underscores(_) as thousands separator and separate every 3 digits with them.";
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+$").unwrap());
static FOUR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{4}").unwrap());
static SHORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_\d{1,2}_").unwrap());
static SHORT_STRICT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_\d{1,2}(_|$)").unwrap());

/// NumericLiterals#format_number.
fn format_number(source: &str) -> String {
    let source: String = source.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let (int_part, rest) = match source.find(['e', 'E', '.']) {
        Some(i) => (&source[..i], &source[i..]),
        None => (source.as_str(), ""),
    };
    // Integer(int_part).abs.to_s grouped by threes from the right, sign kept.
    let digits: String = int_part.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut out = String::new();
    if int_part.starts_with('-') && digits != "0" {
        out.push('-');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('_');
        }
        out.push(ch);
    }
    out + rest
}

impl Cop for NumericLiterals {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INTEGER_NODE, kind::FLOAT_NODE, kind::SOURCE_LINE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `__LINE__` is an int to the parser gem: on_int reads AllowedNumbers (a String raises,
        // #514), and its integer part is never digits.
        if node.as_source_line_node().is_some() {
            cx.cfg.array("AllowedNumbers");
            return;
        }
        let Some((start, end)) = literal_range(node, cx.ancestors) else { return };
        let source = cx.src.slice(start, end);
        let int = integer_part(source);
        if int.starts_with('0') || cx.cfg.array("AllowedNumbers").iter().any(|n| n == int) {
            return;
        }
        // AllowedPattern, anchored: `/\A#{regexp}\z/`.
        let patterns = crate::helpers::allowed::allowed_patterns(cx.cfg);
        if patterns.into_iter().any(|r| {
            crate::config::value::ruby_regex(&format!(r"\A(?:{})\z", r.as_str())).is_some_and(|r| r.is_match(int))
        }) {
            return;
        }
        if (int.chars().count() as i64) < cx.cfg.int("MinDigits").unwrap_or(5) {
            return;
        }
        let short = if cx.cfg.bool("Strict") == Some(true) { &SHORT_STRICT } else { &SHORT };
        if DIGITS.is_match(int) || FOUR.is_match(int) || short.is_match(int) {
            let formatted = format_number(source);
            cx.add_fix(start, end, MSG, |c| c.replace(start, end, &formatted));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats() {
        assert_eq!(super::format_number("-1234567.5e3"), "-1_234_567.5e3");
        assert_eq!(super::format_number("12_34_567"), "1_234_567");
        assert_eq!(super::format_number("- 12345"), "-12_345");
    }
}
