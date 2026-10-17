use crate::prelude::*;

pub const NAME: &str = "RSpec/Dialect";
pub static COP: Dialect = Dialect;
pub struct Dialect;

impl Cop for Dialect {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send #rspec? #ALL.all ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !helpers::rspec::rspec_recv(&call) {
            return;
        }
        let name = method_name(&call);
        if !helpers::rspec::lang(cx).any(name) {
            return;
        }
        let Some(prefer) = helpers::method_preference::preferred_method(cx, name) else { return };
        let (s, e) = send_range(&call);
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let replacement = helpers::method_preference::preferred_method(cx, cx.src.slice(ss, se));
        cx.add_fix(s, e, format!("Prefer `{prefer}` over `{name}`."), |c| {
            if let Some(r) = &replacement {
                c.replace(ss, se, r);
            }
        });
    }
}
