use crate::prelude::*;

pub const NAME: &str = "Style/ArrayJoin";
pub static COP: ArrayJoin = ArrayJoin;
pub struct ArrayJoin;

impl Cop for ArrayJoin {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "*" {
            return;
        }
        // (send $array :* $str)
        let Some(array) = call.receiver().filter(|r| r.as_array_node().is_some()) else { return };
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        if arg.as_string_node().is_none() || is_parser_dstr(arg) {
            return;
        }
        let replacement = format!("{}.join({})", node_text(&array), node_text(arg));
        let (start, end) = send_range(&call);
        let sel = call.message_loc().unwrap();
        cx.add_fix(sel.start_offset(), sel.end_offset(), "Favor `Array#join` over `Array#*`.", |c| {
            c.replace(start, end, &replacement)
        });
    }
}
