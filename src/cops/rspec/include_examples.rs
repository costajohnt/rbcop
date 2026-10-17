use crate::prelude::*;

pub const NAME: &str = "RSpec/IncludeExamples";
pub static COP: IncludeExamples = IncludeExamples;
pub struct IncludeExamples;

impl Cop for IncludeExamples {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "include_examples" {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Prefer `it_behaves_like` over `include_examples`.", |c| c.replace(s, e, "it_behaves_like"));
    }
}
