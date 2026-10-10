use crate::prelude::*;
use helpers::method_preference::preferred_method;

pub const NAME: &str = "Style/CollectionMethods";
pub static COP: CollectionMethods = CollectionMethods;
pub struct CollectionMethods;

impl Cop for CollectionMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let block_literal = call.block().is_some_and(|b| b.as_block_node().is_some());
        // on_send: `implicit_block?`
        let implicit = || {
            if call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
                return true;
            }
            call_args(&call).last().is_some_and(|a| a.as_symbol_node().is_some())
                && cx.cfg.strings("MethodsAcceptingSymbol").iter().any(|m| m == name)
        };
        if !block_literal && !implicit() {
            return;
        }
        let Some(preferred) = preferred_method(cx, name) else { return };
        let Some(sel) = call.message_loc() else { return };
        let (start, end) = (sel.start_offset(), sel.end_offset());
        // `preferred_method(node.loc.selector.source)`
        let replacement = preferred_method(cx, text(&sel)).unwrap_or_default();
        let msg = format!("Prefer `{preferred}` over `{name}`.");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
