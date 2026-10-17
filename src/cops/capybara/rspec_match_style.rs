use crate::prelude::*;

pub const NAME: &str = "Capybara/RSpec/MatchStyle";
pub static COP: MatchStyle = MatchStyle;
pub struct MatchStyle;

impl Cop for MatchStyle {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let good = match method_name(&call) {
            "has_style?" => "matches_style?",
            "have_style" => "match_style",
            _ => return,
        };
        if is_csend(&call) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let bad = text(&sel);
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Use `{good}` instead of `{bad}`."), |c| c.replace(s, e, good));
    }
}
