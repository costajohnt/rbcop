use crate::prelude::*;

pub const NAME: &str = "RSpec/ItBehavesLike";
pub static COP: ItBehavesLike = ItBehavesLike;
pub struct ItBehavesLike;

impl Cop for ItBehavesLike {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "it_behaves_like" | "it_should_behave_like") {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("it_behaves_like").to_string();
        let alternative = if style == "it_behaves_like" { "it_should_behave_like" } else { "it_behaves_like" };
        if name != alternative {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (s, e) = send_range(&call);
        let msg = format!("Prefer `{style}` over `{alternative}` when including examples in a nested context.");
        cx.add_fix(s, e, msg, |c| c.replace(ss, se, &style));
    }
}
