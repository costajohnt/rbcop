use crate::prelude::*;

pub const NAME: &str = "Style/NilComparison";
pub static COP: NilComparison = NilComparison;
pub struct NilComparison;

impl Cop for NilComparison {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || call.receiver().is_none() {
            return;
        }
        let name = method_name(&call);
        let args = call_args(&call);
        let (hit, msg) = if cx.cfg.str("EnforcedStyle") == Some("comparison") {
            // (send _ :nil?)
            (name == "nil?" && args.is_empty(), "Prefer the use of the `==` comparison.")
        } else {
            // (send _ {:== :===} nil)
            ((name == "==" || name == "===") && args.len() == 1 && args[0].as_nil_node().is_some(), "Prefer the use of the `nil?` predicate.")
        };
        if hit && let Some(sel) = call.message_loc() {
            cx.add(sel.start_offset(), sel.end_offset(), msg);
        }
    }
}
