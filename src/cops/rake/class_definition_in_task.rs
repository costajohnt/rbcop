use crate::helpers::rake::in_task_outside_class;
use crate::prelude::*;

pub const NAME: &str = "Rake/ClassDefinitionInTask";
pub static COP: ClassDefinitionInTask = ClassDefinitionInTask;
pub struct ClassDefinitionInTask;

impl Cop for ClassDefinitionInTask {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !in_task_outside_class(cx.ancestors) {
            return;
        }
        let ty = if node.as_class_node().is_some() { "class" } else { "module" };
        let loc = node.location();
        cx.add(
            loc.start_offset(),
            loc.end_offset(),
            format!("Do not define a {ty} in rake task, because it will be defined to the top level."),
        );
    }
}
