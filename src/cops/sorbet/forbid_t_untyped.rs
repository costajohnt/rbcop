use crate::helpers::sorbet_rbs_assertion::is_t_call;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTUntyped";
pub static COP: ForbidTUntyped = ForbidTUntyped;
pub struct ForbidTUntyped;

impl Cop for ForbidTUntyped {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const nil? :T) :untyped)
        let call = node.as_call_node().unwrap();
        if is_t_call(&call, "untyped", Some(0)) {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Do not use `T.untyped`.");
        }
    }
}
