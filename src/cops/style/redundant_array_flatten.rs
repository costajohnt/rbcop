use crate::prelude::*;

pub const NAME: &str = "Style/RedundantArrayFlatten";
pub static COP: RedundantArrayFlatten = RedundantArrayFlatten;
pub struct RedundantArrayFlatten;

impl Cop for RedundantArrayFlatten {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "flatten" || call.receiver().is_none() || send_args(&call).len() > 1 {
            return;
        }
        if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        // (call (call !nil? :flatten _?) :join (nil)?) on the parent
        let Some(parent) = cx.parent().and_then(|p| p.as_call_node()) else { return };
        let loc = node.location();
        if method_name(&parent) != "join"
            || !parent.receiver().is_some_and(|r| r.location().start_offset() == loc.start_offset() && r.location().end_offset() == loc.end_offset())
        {
            return;
        }
        let join_args = send_args(&parent);
        if !(join_args.is_empty() || (join_args.len() == 1 && join_args[0].as_nil_node().is_some())) {
            return;
        }
        let Some(dot) = call.call_operator_loc() else { return };
        let (start, end) = (dot.start_offset(), send_range(&call).1);
        cx.add_fix(start, end, "Remove the redundant `flatten`.", |c| c.remove(start, end));
    }
}
