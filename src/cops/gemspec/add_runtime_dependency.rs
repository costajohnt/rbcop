use crate::prelude::*;

pub const NAME: &str = "Gemspec/AddRuntimeDependency";
pub static COP: AddRuntimeDependency = AddRuntimeDependency;
pub struct AddRuntimeDependency;

impl Cop for AddRuntimeDependency {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "add_runtime_dependency" {
            return;
        }
        if call.receiver().is_none() || send_args(&call).is_empty() {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Use `add_dependency` instead of `add_runtime_dependency`.", |c| {
            c.replace(s, e, "add_dependency")
        });
    }
}
