use crate::prelude::*;

pub const NAME: &str = "Rails/NegateInclude";
pub static COP: NegateInclude = NegateInclude;
pub struct NegateInclude;

impl Cop for NegateInclude {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send $!nil? :include? $_) :!)
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "!" || !send_args(&call).is_empty() {
            return;
        }
        let Some(inner) = call.receiver().and_then(|r| helpers::rails::send(&r)) else { return };
        if method_name(&inner) != "include?" || inner.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let Some(recv) = inner.receiver() else { return };
        let args = send_args(&inner);
        let [obj] = args.as_slice() else { return };
        let (s, e) = send_range(&call);
        let replacement = format!("{}.exclude?({})", node_text(&recv), node_text(obj));
        cx.add_fix(s, e, "Use `.exclude?` and remove the negation part.", |c| c.replace(s, e, &replacement));
    }
}
