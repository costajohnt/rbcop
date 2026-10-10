use crate::prelude::*;

pub const NAME: &str = "Lint/RedundantWithObject";
pub static COP: RedundantWithObject = RedundantWithObject;
pub struct RedundantWithObject;

impl Cop for RedundantWithObject {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let name = method_name(&call);
        if name != "each_with_object" && name != "with_object" {
            return;
        }
        if call_args(&call).len() != 1
            || !helpers::block_args::single_arg_or_numbered(&block, false)
        {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = (sel.start_offset(), send_range(&call).1);
        if name == "each_with_object" {
            cx.add_fix(s, e, "Use `each` instead of `each_with_object`.", |c| c.replace(s, e, "each"));
        } else {
            // `corrector.remove(send.loc.dot)` raises without a dot, losing the offense.
            let Some(dot) = call.call_operator_loc() else { return };
            let (ds, de) = (dot.start_offset(), dot.end_offset());
            cx.add_fix(s, e, "Remove redundant `with_object`.", |c| {
                c.remove(s, e);
                c.remove(ds, de);
            });
        }
    }
}
