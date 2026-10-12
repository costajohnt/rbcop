use crate::prelude::*;

pub const NAME: &str = "Rails/ApplicationController";
pub static COP: ApplicationController = ApplicationController;
pub struct ApplicationController;

impl Cop for ApplicationController {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        helpers::rails::enforce_superclass(cx, node, "ApplicationController", "ActionController::Base", "Controllers should subclass `ApplicationController`.");
    }
}
