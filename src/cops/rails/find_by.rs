use crate::prelude::*;

pub const NAME: &str = "Rails/FindBy";
pub static COP: FindBy = FindBy;
pub struct FindBy;

impl Cop for FindBy {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "first" | "take") || !send_args(&call).is_empty() {
            return;
        }
        // where_method?: a send/csend `where` (not a block).
        let Some(recv) = call.receiver() else { return };
        let Some(where_call) = recv.as_call_node() else { return };
        if where_call.block().is_some_and(|b| b.as_block_node().is_some()) || method_name(&where_call) != "where" {
            return;
        }
        let ignore_first = !matches!(cx.cfg.get("IgnoreWhereFirst"), Some(Value::Bool(false)) | Some(Value::Null));
        if ignore_first && name == "first" {
            return;
        }
        let (Some(wsel), Some(sel)) = (where_call.message_loc(), call.message_loc()) else { return };
        let dot = call.call_operator_loc().map_or("", |d| text(&d));
        let (s, e) = (wsel.start_offset(), sel.end_offset());
        let msg = format!("Use `find_by` instead of `where{dot}{name}`.");
        let (ws, we) = (wsel.start_offset(), wsel.end_offset());
        let recv_end = recv.location().end_offset();
        let take = name == "take";
        cx.add_fix(s, e, msg, |c| {
            if take {
                c.replace(ws, we, "find_by");
                c.replace(recv_end, e, "");
            }
        });
    }
}
