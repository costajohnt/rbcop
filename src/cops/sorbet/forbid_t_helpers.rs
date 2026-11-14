use crate::helpers::sorbet_signature::is_t_const;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTHelpers";
pub static COP: ForbidTHelpers = ForbidTHelpers;
pub struct ForbidTHelpers;

impl Cop for ForbidTHelpers {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "extend" | "include") {
            return;
        }
        if matches!(send_args(&call).as_slice(), [a] if is_t_const(a, "Helpers")) {
            let (s, e) = send_range(&call);
            cx.add(s, e, format!("Do not use `{name} T::Helpers`."));
        }
    }
}
