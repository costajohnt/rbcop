use crate::prelude::*;

pub const NAME: &str = "Rails/ApplicationMailer";
pub static COP: ApplicationMailer = ApplicationMailer;
pub struct ApplicationMailer;

impl Cop for ApplicationMailer {
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
        helpers::rails::enforce_superclass(cx, node, "ApplicationMailer", "ActionMailer::Base", "Mailers should subclass `ApplicationMailer`.");
    }
}
