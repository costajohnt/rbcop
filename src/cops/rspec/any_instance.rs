use crate::prelude::*;

pub const NAME: &str = "RSpec/AnyInstance";
pub static COP: AnyInstance = AnyInstance;
pub struct AnyInstance;

impl Cop for AnyInstance {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "any_instance" | "allow_any_instance_of" | "expect_any_instance_of") {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, format!("Avoid stubbing using `{name}`."));
    }
}
