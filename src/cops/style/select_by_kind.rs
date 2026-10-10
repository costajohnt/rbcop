use crate::helpers::block_args::{single_body, single_var};
use crate::helpers::select_by::{is_block_var, negation_receiver, plain_send, receiver_allowed};
use crate::prelude::*;

pub const NAME: &str = "Style/SelectByKind";
pub static COP: SelectByKind = SelectByKind;
pub struct SelectByKind;

impl Cop for SelectByKind {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        let select = matches!(method, "select" | "filter" | "find_all");
        if !select && method != "reject" {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if receiver_allowed(call.receiver()) {
            return;
        }
        let Some((single, var)) = single_var(&block) else { return };
        let Some(body) = single_body(&block) else { return };
        let (negated, inner) = match negation_receiver(&body) {
            Some(r) => (true, r),
            None => (false, body),
        };
        // (send (lvar %1) {:is_a? :kind_of?} _)
        let Some(check) = plain_send(&inner) else { return };
        if !matches!(method_name(&check), "is_a?" | "kind_of?") {
            return;
        }
        let args = send_args(&check);
        let [class_constant] = args.as_slice() else { return };
        if !check.receiver().is_some_and(|r| is_block_var(&r, single, var)) {
            return;
        }
        let replacement = if select == negated { "grep_v" } else { "grep" };
        let msg = format!("Prefer `{replacement}` to `{method}` with a kind check.");
        let Some(sel) = call.message_loc() else { return };
        let (start, end) = (sel.start_offset(), block.closing_loc().end_offset());
        let fix = format!("{replacement}({})", node_text(class_constant));
        let loc = node.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| c.replace(start, end, &fix));
    }
}
