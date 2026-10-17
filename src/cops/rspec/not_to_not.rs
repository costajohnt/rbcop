use crate::prelude::*;

pub const NAME: &str = "RSpec/NotToNot";
pub static COP: NotToNot = NotToNot;
pub struct NotToNot;

impl Cop for NotToNot {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "not_to" | "to_not") {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("not_to").to_string();
        let alternative = if style == "not_to" { "to_not" } else { "not_to" };
        if name != alternative {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Prefer `{style}` over `{alternative}`."), |c| c.replace(s, e, &style));
    }
}
