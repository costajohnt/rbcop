use crate::prelude::*;

pub const NAME: &str = "Rails/ShortI18n";
pub static COP: ShortI18n = ShortI18n;
pub struct ShortI18n;

impl Cop for ShortI18n {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        let good = match name {
            "translate" => "t",
            "localize" => "l",
            _ => return,
        };
        if cx.cfg.str("EnforcedStyle") == Some("conservative") && call.receiver().is_none() {
            return;
        }
        if !call.receiver().is_none_or(|r| helpers::rails::is_const(&r, "I18n")) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Use `{good}` instead of `{name}`."), |c| c.replace(s, e, good));
    }
}
