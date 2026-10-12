use crate::prelude::*;

pub const NAME: &str = "Performance/MethodObjectAsBlock";
pub static COP: MethodObjectAsBlock = MethodObjectAsBlock;
pub struct MethodObjectAsBlock;

impl Cop for MethodObjectAsBlock {
    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_ARGUMENT_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (^send (send _ :method sym))
        let Some(parent) = cx.parent().and_then(|p| p.as_call_node()) else { return };
        if is_csend(&parent) {
            return;
        }
        let Some(call) = node.as_block_argument_node().unwrap().expression().and_then(|e| e.as_call_node()) else {
            return;
        };
        let target = cx.config.target_ruby;
        let sym =
            matches!(send_args(&call).as_slice(), [a] if a.as_symbol_node().is_some() && !is_parser_dsym(a, target));
        if is_csend(&call) || method_name(&call) != "method" || call.block().is_some() || !sym {
            return;
        }
        let loc = node.location();
        cx.add(loc.start_offset(), loc.end_offset(), "Use block explicitly instead of block-passing a method object.");
    }
}
