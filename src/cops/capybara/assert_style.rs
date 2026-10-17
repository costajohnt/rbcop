use crate::prelude::*;

pub const NAME: &str = "Capybara/AssertStyle";
pub static COP: AssertStyle = AssertStyle;
pub struct AssertStyle;

impl Cop for AssertStyle {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "assert_style" {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Use `assert_matches_style` instead of `assert_style`.", |c| c.replace(s, e, "assert_matches_style"));
    }
}
