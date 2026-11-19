use crate::helpers::rake::in_task_outside_class;
use crate::prelude::*;

pub const NAME: &str = "Rake/MethodDefinitionInTask";
pub static COP: MethodDefinitionInTask = MethodDefinitionInTask;
pub struct MethodDefinitionInTask;

impl Cop for MethodDefinitionInTask {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if in_task_outside_class(cx.ancestors) {
            let loc = node.location();
            cx.add(
                loc.start_offset(),
                loc.end_offset(),
                "Do not define a method in rake task, because it will be defined to the top level.",
            );
        }
    }
}
