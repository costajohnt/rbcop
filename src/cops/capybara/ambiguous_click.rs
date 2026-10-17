use crate::prelude::*;

pub const NAME: &str = "Capybara/AmbiguousClick";
pub static COP: AmbiguousClick = AmbiguousClick;
pub struct AmbiguousClick;

impl Cop for AmbiguousClick {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "click_link_or_button" | "click_on") {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, format!("Use `click_link` or `click_button` instead of `{name}`."));
    }
}
