use crate::helpers::block_args::{single_body, single_var};
use crate::helpers::select_by::{is_block_var, negation_receiver, paren_single, plain_send, receiver_allowed};
use crate::prelude::*;

pub const NAME: &str = "Style/SelectByRange";
pub static COP: SelectByRange = SelectByRange;
pub struct SelectByRange;

impl Cop for SelectByRange {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        let (select, find) = match method {
            "select" | "filter" | "find_all" => (true, false),
            "find" | "detect" => (false, true),
            "reject" => (false, false),
            _ => return,
        };
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if receiver_allowed(call.receiver()) {
            return;
        }
        let Some((single, var)) = single_var(&block) else { return };
        let Some(body) = single_body(&block) else { return };
        let (negated, inner) = match negation_receiver(&body) {
            Some(r) => (true, paren_single(&r).unwrap_or(r)),
            None => (false, body),
        };
        let Some(check) = plain_send(&inner) else { return };
        let args = send_args(&check);
        let range_literal = match (method_name(&check), args.as_slice()) {
            // (send (lvar %1) :between? _ _)
            ("between?", [min, max]) if check.receiver().is_some_and(|r| is_block_var(&r, single, var)) => {
                format!("{}..{}", node_text(min), node_text(max))
            }
            // (send {range (begin range)} {:cover? :include?} (lvar %1))
            ("cover?" | "include?", [arg]) if is_block_var(arg, single, var) => {
                let Some(recv) = check.receiver() else { return };
                let range = if recv.as_range_node().is_some() { Some(recv) } else { paren_single(&recv) };
                let Some(range) = range.filter(|r| r.as_range_node().is_some()) else { return };
                node_text(&range).to_string()
            }
            _ => return,
        };
        let grep = if (!select && !find) != negated { "grep_v" } else { "grep" };
        let replacement = if find { format!("{grep}(...).first") } else { grep.to_string() };
        let msg = format!("Prefer `{replacement}` to `{method}` with a range check.");
        let Some(sel) = call.message_loc() else { return };
        let (start, end) = (sel.start_offset(), block.closing_loc().end_offset());
        let suffix = if find { ".first" } else { "" };
        let fix = format!("{grep}({range_literal}){suffix}");
        let loc = node.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| c.replace(start, end, &fix));
    }
}
