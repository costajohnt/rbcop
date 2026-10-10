use crate::prelude::*;

pub const NAME: &str = "Style/ExponentialNotation";
pub static COP: ExponentialNotation = ExponentialNotation;
pub struct ExponentialNotation;

static SCIENTIFIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[1-9](\.\d*[0-9])?$").unwrap());
static INTEGRAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[1-9](\d*[1-9])?$").unwrap());
static INT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d+$").unwrap());
static FOUR_DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d{4}").unwrap());
static LEADING_ZERO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?0\d").unwrap());
static ZERO_POINT_ZERO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?0.0").unwrap());

impl Cop for ExponentialNotation {
    fn kinds(&self) -> &'static [usize] {
        &[kind::FLOAT_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `1e5i` is a parser `complex`, not a `float`.
        if cx.ancestors.last().is_some_and(|p| p.as_imaginary_node().is_some()) {
            return;
        }
        let source = node_text(node);
        if !source.contains('e') {
            return;
        }
        let mut parts = source.split('e');
        let mantissa = parts.next().unwrap_or("");
        let exponent = parts.next().unwrap_or("");
        let (ok, msg) = match cx.cfg.str("EnforcedStyle") {
            Some("scientific") => (SCIENTIFIC.is_match(mantissa), "Use a mantissa >= 1 and < 10."),
            Some("engineering") => (
                INT.is_match(exponent)
                    && exponent.parse::<i64>().is_ok_and(|e| e % 3 == 0)
                    && !FOUR_DIGITS.is_match(mantissa)
                    && !LEADING_ZERO.is_match(mantissa)
                    && !ZERO_POINT_ZERO.is_match(mantissa),
                "Use an exponent divisible by 3 and a mantissa >= 0.1 and < 1000.",
            ),
            Some("integral") => (INTEGRAL.is_match(mantissa), "Use an integer as mantissa, without trailing zero."),
            _ => return,
        };
        if !ok {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), msg);
        }
    }
}
