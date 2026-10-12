use crate::prelude::*;

pub const NAME: &str = "Rails/UniqBeforePluck";
pub static COP: UniqBeforePluck = UniqBeforePluck;
pub struct UniqBeforePluck;

impl Cop for UniqBeforePluck {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // [!^any_block $(send $(send _ :pluck ...) :uniq ...)]
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "uniq" || call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let anc = cx.ancestors;
        if helpers::rails::parent_index(anc).is_some_and(|i| matches!(anc[i], Node::BlockNode { .. } | Node::LambdaNode { .. })) {
            return;
        }
        let Some(pluck) = call.receiver().and_then(|r| helpers::rails::send_node(&r)) else { return };
        if method_name(&pluck) != "pluck" {
            return;
        }
        if cx.cfg.str("EnforcedStyle") != Some("aggressive")
            && !pluck.receiver().is_some_and(|r| r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some())
        {
            return;
        }
        // The corrector reads pluck's `loc.end` (its closing parenthesis); without one it raises.
        let Some(close) = pluck.closing_loc() else { return };
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let close_end = close.end_offset();
        let dot = pluck.call_operator_loc().map(|d| d.start_offset());
        let pluck_range = (pluck.location().start_offset(), pluck.location().end_offset());
        cx.add_fix(s, e, "Use `distinct` before `pluck`.", |c| {
            c.remove(close_end, e);
            match dot {
                Some(d) => c.insert_before(d, d, ".distinct"),
                None => c.insert_before(pluck_range.0, pluck_range.1, "distinct."),
            }
        });
    }
}
