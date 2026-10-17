use crate::prelude::*;

pub const NAME: &str = "RSpec/BeNil";
pub static COP: BeNil = BeNil;
pub struct BeNil;

impl Cop for BeNil {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rspec::send_call(node).filter(|c| c.receiver().is_none()) else { return };
        let args = send_args(&call);
        let (s, e) = send_range(&call);
        match cx.cfg.str("EnforcedStyle") {
            // (send nil? :be_nil)
            Some("be") if method_name(&call) == "be_nil" && args.is_empty() => {
                cx.add_fix(s, e, "Prefer `be(nil)` over `be_nil`.", |c| c.replace(s, e, "be(nil)"));
            }
            // (send nil? :be nil)
            Some("be_nil") if method_name(&call) == "be" && args.len() == 1 && args[0].as_nil_node().is_some() => {
                cx.add_fix(s, e, "Prefer `be_nil` over `be(nil)`.", |c| c.replace(s, e, "be_nil"));
            }
            _ => {}
        }
    }
}
