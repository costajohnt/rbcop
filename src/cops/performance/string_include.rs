use crate::helpers::regexp_metacharacter::{interpret_string_escapes, literal, redundant_regex};
use crate::prelude::*;

pub const NAME: &str = "Performance/StringInclude";
pub static COP: StringInclude = StringInclude;
pub struct StringInclude;

impl Cop for StringInclude {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some((receiver, regex)) =
            redundant_regex(&call, &["match", "=~", "!~", "match?"], &["match", "match?", "==="], literal)
        else {
            return;
        };
        let negation = method_name(&call) == "!~" && !is_csend(&call);
        let bang = if negation { "!" } else { "" };
        let msg = format!("Use `{bang}String#include?` instead of a regex match with literal-only pattern.");
        // `/re/ =~ x` is a match-with-lvasgn, which has no dot.
        let dot = call.call_operator_loc().map_or(".", |d| text(&d));
        let value = helpers::percent_array::safe_string_literal(&interpret_string_escapes(regex));
        let new_source = format!("{bang}{}{dot}include?({value})", node_text(&receiver));
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &new_source));
    }
}
