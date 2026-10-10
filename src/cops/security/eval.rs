use crate::prelude::*;

pub const NAME: &str = "Security/Eval";
pub static COP: Eval = Eval;
pub struct Eval;

impl Cop for Eval {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "eval" {
            return;
        }
        // (send {nil? (send nil? :binding) (const {cbase nil?} :Kernel)} :eval $!str ...)
        let recv_ok = call.receiver().is_none_or(|r| match r.as_call_node() {
            Some(b) => {
                method_name(&b) == "binding" && b.receiver().is_none() && b.arguments().is_none() && b.block().is_none()
            }
            None => helpers::const_name::const_name(&r).as_deref() == Some("Kernel"),
        });
        let Some(code) = send_args(&call).into_iter().next() else { return };
        if !recv_ok || code.as_source_file_node().is_some() || (code.as_string_node().is_some() && !is_parser_dstr(&code)) {
            return;
        }
        if is_parser_dstr(&code) && helpers::literal_node::recursive_basic_literal(&code) {
            return;
        }
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), "The use of `eval` is a serious security risk.");
    }
}
