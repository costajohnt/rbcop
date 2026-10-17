use crate::prelude::*;

pub const NAME: &str = "RSpec/ContextMethod";
pub static COP: ContextMethod = ContextMethod;
pub struct ContextMethod;

impl Cop for ContextMethod {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send #rspec? :context ${(str #method_name?) (dstr (str #method_name?) ...)} ...) ...)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if method_name(&call) != "context" || !helpers::rspec::rspec_recv(&call) {
            return;
        }
        let Some(arg) = send_args(&call).into_iter().next() else { return };
        if !helpers::rspec::str_or_dstr_head(&arg).is_some_and(|v| v.starts_with(['.', '#'])) {
            return;
        }
        let loc = arg.location();
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(loc.start_offset(), loc.end_offset(), "Use `describe` for testing methods.", |c| {
            c.replace(ss, se, "describe")
        });
    }
}
