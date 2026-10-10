use crate::prelude::*;

pub const NAME: &str = "Lint/RedundantWithIndex";
pub static COP: RedundantWithIndex = RedundantWithIndex;
pub struct RedundantWithIndex;

impl Cop for RedundantWithIndex {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let Some(recv) = call.receiver() else { return };
        let name = method_name(&call);
        if name != "each_with_index" && name != "with_index" {
            return;
        }
        if name == "with_index" && recv.as_call_node().and_then(|r| r.receiver()).is_none() {
            return;
        }
        if !helpers::block_args::single_arg_or_numbered(&block, true) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), send_range(&call).1);
        let dot = call.call_operator_loc().map(|l| (l.start_offset(), l.end_offset()));
        if name == "each_with_index" {
            let sel_end = sel.end_offset();
            cx.add_fix(s, e, "Use `each` instead of `each_with_index`.", |c| c.replace(s, sel_end, "each"));
        } else {
            cx.add_fix(s, e, "Remove redundant `with_index`.", |c| {
                c.remove(s, e);
                if let Some((ds, de)) = dot {
                    c.remove(ds, de);
                }
            });
        }
    }
}
