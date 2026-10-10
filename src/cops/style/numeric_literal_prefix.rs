use crate::helpers::integer_node::{integer_part, literal_range};
use crate::prelude::*;

pub const NAME: &str = "Style/NumericLiteralPrefix";
pub static COP: NumericLiteralPrefix = NumericLiteralPrefix;
pub struct NumericLiteralPrefix;

static OCTAL_ZERO_ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^0[Oo][0-7]+$").unwrap());
static OCTAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^0O?[0-7]+$").unwrap());
static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^0X[0-9A-F]+$").unwrap());
static BINARY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^0B[01]+$").unwrap());
static DECIMAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^0[dD][0-9]+$").unwrap());

impl Cop for NumericLiteralPrefix {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INTEGER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((start, end)) = literal_range(node, cx.ancestors) else { return };
        let src = cx.src;
        let source = src.slice(start, end);
        let literal = integer_part(source);
        let zero_only = cx.cfg.str("EnforcedOctalStyle") == Some("zero_only");
        // (message, prefix length replaced, new prefix)
        let (msg, from, to) = if OCTAL_ZERO_ONLY.is_match(literal) && zero_only {
            ("Use 0 for octal literals.", 2, "0")
        } else if OCTAL.is_match(literal) && !zero_only {
            ("Use 0o for octal literals.", if literal.starts_with("0O") { 2 } else { 1 }, "0o")
        } else if HEX.is_match(literal) {
            ("Use 0x for hexadecimal literals.", 2, "0x")
        } else if BINARY.is_match(literal) {
            ("Use 0b for binary literals.", 2, "0b")
        } else if DECIMAL.is_match(literal) {
            ("Do not use prefixes for decimal literals.", 2, "")
        } else {
            return;
        };
        // unsigned_range: the node minus a leading sign.
        let ustart = if source.starts_with(['+', '-']) { start + 1 } else { start };
        let replacement = format!("{to}{}", &src.slice(ustart, end)[from..]);
        cx.add_fix(start, end, msg, |c| c.replace(ustart, end, &replacement));
    }
}
