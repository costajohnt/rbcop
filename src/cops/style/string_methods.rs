use crate::prelude::*;
use helpers::method_preference::preferred_method;

pub const NAME: &str = "Style/StringMethods";
pub static COP: StringMethods = StringMethods;
pub struct StringMethods;

impl Cop for StringMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let Some(preferred) = preferred_method(cx, name) else { return };
        let Some(sel) = call.message_loc() else { return };
        let (start, end) = (sel.start_offset(), sel.end_offset());
        let msg = format!("Prefer `{preferred}` over `{name}`.");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &preferred));
    }
}
