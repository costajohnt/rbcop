use crate::helpers::sorbet_rbs_assertion::{is_t_call, rbs_assertion_edits};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTUnsafe";
pub static COP: ForbidTUnsafe = ForbidTUnsafe;
pub struct ForbidTUnsafe;

impl Cop for ForbidTUnsafe {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const nil? :T) :unsafe _)
        let call = node.as_call_node().unwrap();
        if !is_t_call(&call, "unsafe", Some(1)) {
            return;
        }
        let args = send_args(&call);
        let edits = rbs_assertion_edits(cx, &call, true, || Some((node_text(&args[0]).to_string(), "as untyped".to_string())));
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Do not use `T.unsafe`.", |c| {
            for (a, b, t) in &edits {
                c.replace(*a, *b, t);
            }
        });
    }
}
