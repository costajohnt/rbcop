use crate::prelude::*;

pub const NAME: &str = "Rails/ScopeArgs";
pub static COP: ScopeArgs = ScopeArgs;
pub struct ScopeArgs;

impl Cop for ScopeArgs {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :scope _ $send)
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "scope") {
            return;
        }
        let args = send_args(&call);
        let [_, second] = args.as_slice() else { return };
        // A parser `send`: a call without `&.` and without a literal block.
        let Some(c) = helpers::rails::send(second) else { return };
        if c.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let (s, e) = (second.location().start_offset(), second.location().end_offset());
        let replacement = format!("-> {{ {} }}", node_text(second));
        cx.add_fix(s, e, "Use `lambda`/`proc` instead of a plain method call.", |c| c.replace(s, e, &replacement));
    }
}
