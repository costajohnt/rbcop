use crate::prelude::*;

pub const NAME: &str = "Style/RedundantArrayConstructor";
pub static COP: RedundantArrayConstructor = RedundantArrayConstructor;
pub struct RedundantArrayConstructor;

impl Cop for RedundantArrayConstructor {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        let single_array = || args.len() == 1 && args[0].as_array_node().is_some();
        let array_receiver = call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Array"));
        let (send_start, send_end) = send_range(&call);
        let ((start, end), replacement) = match method_name(&call) {
            "new" if array_receiver && single_array() => {
                let sel = call.message_loc().unwrap();
                ((send_start, sel.end_offset()), args[0].location())
            }
            "Array" if call.receiver().is_none() && single_array() => {
                let Some(sel) = call.message_loc() else { return };
                ((sel.start_offset(), sel.end_offset()), args[0].location())
            }
            "[]" if array_receiver => {
                let (Some(recv), Some(open)) = (call.receiver(), call.opening_loc()) else { return };
                let r = recv.location();
                let text = &cx.src.text[open.start_offset()..send_end];
                let text = text.to_string();
                return cx.add_fix(r.start_offset(), r.end_offset(), "Remove the redundant `Array` constructor.", |c| {
                    c.replace(send_start, send_end, &text)
                });
            }
            _ => return,
        };
        let text = text(&replacement).to_string();
        cx.add_fix(start, end, "Remove the redundant `Array` constructor.", |c| c.replace(send_start, send_end, &text));
    }
}
