use crate::prelude::*;

pub const NAME: &str = "Rails/RequireDependency";
pub static COP: RequireDependency = RequireDependency;
pub struct RequireDependency;

impl Cop for RequireDependency {
    fn min_rails(&self) -> f64 {
        6.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send {nil? (const {nil? cbase} :Kernel)} :require_dependency _)
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "require_dependency"
            || !call.receiver().is_none_or(|r| helpers::rails::is_const(&r, "Kernel"))
            || send_args(&call).len() != 1
        {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Do not use `require_dependency` with Zeitwerk mode.");
    }
}
