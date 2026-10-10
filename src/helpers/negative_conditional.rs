//! RuboCop's NegativeConditional mixin (Style/NegatedIf, Style/NegatedWhile).

use crate::prelude::*;

/// `check_negative_conditional` minus the `if ... else` exclusion: the condition, with parentheses
/// stripped, is `(send !(send _ :!) :!)`.
pub fn single_negative(predicate: Node) -> bool {
    let not_call = |n: &Node| {
        n.as_call_node().is_some_and(|c| !is_csend(&c) && method_name(&c) == "!" && c.arguments().is_none())
    };
    strip_parens(predicate).is_some_and(|c| {
        not_call(&c) && !c.as_call_node().unwrap().receiver().is_some_and(|r| not_call(&r))
    })
}

pub fn message(inverse: &str, current: &str) -> String {
    format!("Favor `{inverse}` over `{current}` for negative conditions.")
}
