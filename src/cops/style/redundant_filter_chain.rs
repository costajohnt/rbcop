use crate::prelude::*;

pub const NAME: &str = "Style/RedundantFilterChain";
pub static COP: RedundantFilterChain = RedundantFilterChain;
pub struct RedundantFilterChain;

impl Cop for RedundantFilterChain {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let replacement = match name {
            "any?" | "present?" => "any?",
            "empty?" | "none?" => "none?",
            "one?" => "one?",
            "many?" => "many?",
            _ => return,
        };
        if !send_args(&call).is_empty() || call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let Some(select) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        let select_name = method_name(&select);
        if !matches!(select_name, "select" | "filter" | "find_all") {
            return;
        }
        let ok = match select.block() {
            Some(b) if b.as_block_node().is_some() => true,
            Some(_) => call_args(&select).is_empty(),
            None => false,
        };
        if !ok {
            return;
        }
        if matches!(name, "many?" | "present?") && !cx.config.all_cops().truthy("ActiveSupportExtensionsEnabled") {
            return;
        }
        let (Some(sel), Some(pred_sel)) = (select.message_loc(), call.message_loc()) else { return };
        let recv_end = select.location().end_offset();
        let msg = format!("Use `{replacement}` instead of `{select_name}.{name}`.");
        let (ss, se, pe) = (sel.start_offset(), sel.end_offset(), pred_sel.end_offset());
        cx.add_fix(ss, pe, msg, |c| {
            c.remove(recv_end, pe);
            c.replace(ss, se, replacement);
        });
    }
}
