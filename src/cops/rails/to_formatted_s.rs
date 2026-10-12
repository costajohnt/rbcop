use crate::prelude::*;

pub const NAME: &str = "Rails/ToFormattedS";
pub static COP: ToFormattedS = ToFormattedS;
pub struct ToFormattedS;

impl Cop for ToFormattedS {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        7.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "to_formatted_s" | "to_fs") {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("to_fs").to_string();
        if name == style {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Use `{style}` instead."), |c| c.replace(s, e, &style));
    }
}
