use crate::prelude::*;

pub const NAME: &str = "Rails/PluckInWhere";
pub static COP: PluckInWhere = PluckInWhere;
pub struct PluckInWhere;

impl Cop for PluckInWhere {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "pluck" | "ids") || !helpers::rails::in_where(cx.ancestors, send_range(&call)) {
            return;
        }
        if cx.cfg.str("EnforcedStyle") != Some("aggressive") {
            // root_receiver: follow call receivers (not blocks) to the first non-call.
            let mut r = call.receiver();
            while let Some(c) = r.as_ref().and_then(|n| n.as_call_node()).filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none())) {
                r = c.receiver();
            }
            if !r.is_some_and(|n| n.as_constant_read_node().is_some() || n.as_constant_path_node().is_some()) {
                return;
            }
        }
        let (msg, replacement) = if name == "ids" {
            ("Use `select(:id)` instead of `ids` within `where` query method.", "select(:id)")
        } else {
            ("Use `select` instead of `pluck` within `where` query method.", "select")
        };
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, msg, |c| c.replace(s, e, replacement));
    }
}
