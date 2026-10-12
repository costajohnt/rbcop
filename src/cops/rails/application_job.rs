use crate::prelude::*;

pub const NAME: &str = "Rails/ApplicationJob";
pub static COP: ApplicationJob = ApplicationJob;
pub struct ApplicationJob;

impl Cop for ApplicationJob {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::rails::enforce_superclass(cx, node, "ApplicationJob", "ActiveJob::Base", "Jobs should subclass `ApplicationJob`.");
    }
}
