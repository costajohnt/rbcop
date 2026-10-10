use crate::prelude::*;

pub const NAME: &str = "Style/ClassCheck";
pub static COP: ClassCheck = ClassCheck;
pub struct ClassCheck;

impl Cop for ClassCheck {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "is_a?" | "kind_of?") || cx.cfg.str("EnforcedStyle") == Some(name) {
            return;
        }
        let (prefer, current) = if name == "is_a?" { ("kind_of?", "is_a?") } else { ("is_a?", "kind_of?") };
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Prefer `Object#{prefer}` over `Object#{current}`."), |c| c.replace(s, e, prefer));
    }
}
