use crate::prelude::*;

pub const NAME: &str = "RSpec/VerifiedDoubles";
pub static COP: VerifiedDoubles = VerifiedDoubles;
pub struct VerifiedDoubles;

impl Cop for VerifiedDoubles {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? {:double :spy} $...)
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || !matches!(method_name(&call), "double" | "spy") {
            return;
        }
        let name = send_args(&call).into_iter().next();
        let nameless = name.as_ref().is_none_or(|n| n.as_hash_node().is_some() || n.as_keyword_hash_node().is_some());
        if nameless && cx.cfg.bool("IgnoreNameless") == Some(true) {
            return;
        }
        if name.as_ref().is_some_and(|n| helpers::rspec::is_sym(cx, n)) && cx.cfg.bool("IgnoreSymbolicNames") == Some(true) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Prefer using verifying doubles over normal doubles.");
    }
}
