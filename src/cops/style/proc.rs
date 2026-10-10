use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/Proc";
pub static COP: Proc = Proc;
pub struct Proc;

impl Cop for Proc {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (any_block $(send (const {nil? cbase} :Proc) :new) ...)
        let call = node.as_call_node().unwrap();
        if call.block().and_then(|b| b.as_block_node()).is_none()
            || is_csend(&call)
            || method_name(&call) != "new"
            || call.arguments().is_some()
            || !call.receiver().is_some_and(|r| is_top_const(&r, "Proc"))
        {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use `proc` instead of `Proc.new`.", |c| c.replace(s, e, "proc"));
    }
}
