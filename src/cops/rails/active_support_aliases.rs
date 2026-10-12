use crate::prelude::*;

pub const NAME: &str = "Rails/ActiveSupportAliases";
pub static COP: ActiveSupportAliases = ActiveSupportAliases;
pub struct ActiveSupportAliases;

impl Cop for ActiveSupportAliases {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let Some(recv) = call.receiver() else { return };
        let str = recv.as_string_node().is_some() && !is_parser_dstr(&recv);
        let array = recv.as_array_node().is_some();
        let prefer = match name {
            "starts_with?" if str => "start_with?",
            "ends_with?" if str => "end_with?",
            "append" if array => "<<",
            "prepend" if array => "unshift",
            _ => return,
        };
        if send_args(&call).len() != 1 {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (_, e) = send_range(&call);
        let msg = format!("Use `{prefer}` instead of `{name}`.");
        if name == "append" {
            cx.add_with(ss, e, msg, false);
            return;
        }
        cx.add_fix(ss, e, msg, |c| c.replace(ss, se, prefer));
    }
}
