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

/// ConditionCorrector.correct_negative_condition: swap the keyword for its inverse and replace the
/// negated condition (parentheses stripped) with its receiver's source.
pub fn correct_negative_condition(c: &mut Corrector, keyword: (usize, usize), inverse: &str, predicate: Node) {
    let Some(cond) = strip_parens(predicate) else { return };
    let call = cond.as_call_node().unwrap();
    let Some(recv) = call.receiver() else { return };
    let (s, e) = send_range(&call);
    c.replace(keyword.0, keyword.1, inverse);
    c.replace(s, e, node_text(&recv));
}
