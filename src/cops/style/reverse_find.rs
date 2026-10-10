use crate::prelude::*;

pub const NAME: &str = "Style/ReverseFind";
pub static COP: ReverseFind = ReverseFind;
pub struct ReverseFind;

impl Cop for ReverseFind {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (4.0, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call (call _ {:reverse :reverse_each}) {:find :detect} (block_pass sym)?)
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "find" | "detect") || call.arguments().is_some() {
            return;
        }
        if let Some(b) = call.block().and_then(|b| b.as_block_argument_node())
            && !b.expression().is_some_and(|e| e.as_symbol_node().is_some())
        {
            return;
        }
        let Some(inner) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if !matches!(method_name(&inner), "reverse" | "reverse_each") || !send_args(&inner).is_empty() {
            return;
        }
        if inner.block().is_some() || inner.equal_loc().is_some() {
            return;
        }
        let (Some(inner_sel), Some(sel)) = (inner.message_loc(), call.message_loc()) else { return };
        let (start, end) = (inner_sel.start_offset(), sel.end_offset());
        cx.add_fix(start, end, "Use `rfind` instead.", |c| c.replace(start, end, "rfind"));
    }
}
