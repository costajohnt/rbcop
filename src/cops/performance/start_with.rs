use crate::helpers::regexp_metacharacter::{
    drop_start, interpret_string_escapes, literal_at_start, redundant_regex, safe_multiline,
};
use crate::prelude::*;

pub const NAME: &str = "Performance/StartWith";
pub static COP: StartWith = StartWith;
pub struct StartWith;

impl Cop for StartWith {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let safe = safe_multiline(cx);
        let Some((receiver, regex)) =
            redundant_regex(&call, &["match", "=~", "match?"], &["match", "match?"], |s| literal_at_start(s, safe))
        else {
            return;
        };
        let dot = call.call_operator_loc().map_or(".", |d| text(&d));
        let value = helpers::percent_array::safe_string_literal(&interpret_string_escapes(drop_start(regex)));
        let new_source = format!("{}{dot}start_with?({value})", node_text(&receiver));
        let (start, end) = send_range(&call);
        cx.add_fix(
            start,
            end,
            "Use `String#start_with?` instead of a regex match anchored to the beginning of the string.",
            |c| c.replace(start, end, &new_source),
        );
    }
}
