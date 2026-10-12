use crate::helpers::regexp_metacharacter::{
    drop_end, interpret_string_escapes, literal_at_end, redundant_regex, safe_multiline,
};
use crate::prelude::*;

pub const NAME: &str = "Performance/EndWith";
pub static COP: EndWith = EndWith;
pub struct EndWith;

impl Cop for EndWith {
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
            redundant_regex(&call, &["match", "=~", "match?"], &["match", "match?"], |s| literal_at_end(s, safe))
        else {
            return;
        };
        let dot = call.call_operator_loc().map_or(".", |d| text(&d));
        let value = helpers::percent_array::safe_string_literal(&interpret_string_escapes(drop_end(regex)));
        let new_source = format!("{}{dot}end_with?({value})", node_text(&receiver));
        let (start, end) = send_range(&call);
        cx.add_fix(
            start,
            end,
            "Use `String#end_with?` instead of a regex match anchored to the end of the string.",
            |c| c.replace(start, end, &new_source),
        );
    }
}
