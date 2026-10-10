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
