use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/CollectionQuerying";
pub static COP: CollectionQuerying = CollectionQuerying;
pub struct CollectionQuerying;

impl Cop for CollectionQuerying {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "positive?" | ">" | "!=" | "zero?" | "==") || is_csend(&call) {
            return;
        }
        if call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
            return;
        }
        // `{(any_block $(call !nil? :count) _ _) $(call !nil? :count (block-pass _)?)}`
        let Some(count) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&count) != "count" || count.receiver().is_none() || count.arguments().is_some() {
            return;
        }
        let Some(count_sel) = count.message_loc() else { return };
        let args = call_args(&call);
        let arg = match args.as_slice() {
            [] => None,
            [a] => match int_literal_value(a) {
                Some(v) => Some(v),
                None => return,
            },
            _ => return,
        };
        let block_given = count.block().is_some();
        let replacement = match (name, arg, block_given) {
            ("positive?", None, true) | (">", Some(0), true) | ("!=", Some(0), true) => "any?",
            ("zero?", None, true) | ("==", Some(0), true) => "none?",
            ("==", Some(1), true) => "one?",
            (">", Some(1), true) => "many?",
            ("positive?", None, false) | (">", Some(0), false) | ("!=", Some(0), false) => "!empty?",
            ("zero?", None, false) | ("==", Some(0), false) => "empty?",
            _ => return,
        };
        if replacement == "many?" && !cx.config.all_cops().truthy("ActiveSupportExtensionsEnabled") {
            return;
        }
        let negated = replacement.starts_with('!');
        // receiver_of_call?: `!x.empty?.foo` would negate the whole chain.
        let start = node.location().start_offset();
        if negated
            && cx.parent().and_then(|p| p.as_call_node()).is_some_and(|p| {
                p.call_operator_loc().is_some() && p.receiver().is_some_and(|r| r.location().start_offset() == start)
            })
        {
            return;
        }
        let (_, end) = send_range(&call);
        let (cs, ce) = (count_sel.start_offset(), count_sel.end_offset());
        let (count_start, count_end) = (count.location().start_offset(), count.location().end_offset());
        let from = call.call_operator_loc().or(call.message_loc()).map_or(end, |l| l.start_offset());
        let left = Surrounding { side: Side::Left, ..Default::default() };
        let (rs, re) = range_with_surrounding_space(cx.src.text, from, end, left);
        cx.add_fix(cs, end, format!("Use `{replacement}` instead."), |c| {
            if negated {
                c.insert_before(count_start, count_end, "!");
                c.replace(cs, ce, "empty?");
            } else {
                c.replace(cs, ce, replacement);
            }
            c.remove(rs, re);
        });
    }
}
