use crate::prelude::*;

pub const NAME: &str = "Capybara/RSpec/HaveContent";
pub static COP: HaveContent = HaveContent;
pub struct HaveContent;

impl Cop for HaveContent {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_send and on_csend
        let call = node.as_call_node().unwrap();
        let good = match method_name(&call) {
            "have_content" => "have_text",
            "have_no_content" => "have_no_text",
            _ => return,
        };
        let Some(sel) = call.message_loc() else { return };
        let bad = text(&sel);
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Prefer `{good}` over `{bad}`."), |c| c.replace(s, e, good));
    }
}
