use crate::helpers::sorbet_signature::is_t_const;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTSig";
pub static COP: ForbidTSig = ForbidTSig;
pub struct ForbidTSig;

impl Cop for ForbidTSig {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "extend" | "include") {
            return;
        }
        if matches!(send_args(&call).as_slice(), [a] if is_t_const(a, "Sig")) {
            let (s, e) = send_range(&call);
            cx.add(s, e, format!("Do not use `{name} T::Sig`."));
        }
    }
}
