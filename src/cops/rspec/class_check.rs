use crate::prelude::*;

pub const NAME: &str = "RSpec/ClassCheck";
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
        if is_csend(&call) || !matches!(name, "be_a" | "be_a_kind_of" | "be_an" | "be_kind_of") {
            return;
        }
        let be_a = cx.cfg.str("EnforcedStyle") == Some("be_a");
        let (preferred, names): (&str, &[&str]) =
            if be_a { ("be_a", &["be_a", "be_an"]) } else { ("be_kind_of", &["be_a_kind_of", "be_kind_of"]) };
        if call.receiver().is_some() || names.contains(&name) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Prefer `{preferred}` over `{name}`."), |c| c.replace(s, e, preferred));
    }
}
