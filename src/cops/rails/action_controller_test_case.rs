use crate::prelude::*;

pub const NAME: &str = "Rails/ActionControllerTestCase";
pub static COP: ActionControllerTestCase = ActionControllerTestCase;
pub struct ActionControllerTestCase;

impl Cop for ActionControllerTestCase {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        let path = class.constant_path();
        if path.as_constant_read_node().is_none() && path.as_constant_path_node().is_none() {
            return;
        }
        let Some(sup) = class.superclass().filter(|s| helpers::rails::is_const(s, "ActionController::TestCase")) else {
            return;
        };
        let (s, e) = (sup.location().start_offset(), sup.location().end_offset());
        cx.add_fix(s, e, "Use `ActionDispatch::IntegrationTest` instead.", |c| {
            c.replace(s, e, "ActionDispatch::IntegrationTest")
        });
    }
}
