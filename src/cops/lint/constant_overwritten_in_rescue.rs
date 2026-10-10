use crate::prelude::*;

pub const NAME: &str = "Lint/ConstantOverwrittenInRescue";
pub static COP: ConstantOverwrittenInRescue = ConstantOverwrittenInRescue;
pub struct ConstantOverwrittenInRescue;

impl Cop for ConstantOverwrittenInRescue {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (resbody nil? $(casgn _ _) nil?)
        let r = node.as_rescue_node().unwrap();
        if !r.exceptions().is_empty() || r.statements().is_some() {
            return;
        }
        let Some(reference) = r.reference() else { return };
        if !matches!(kind::of(&reference), kind::CONSTANT_TARGET_NODE | kind::CONSTANT_PATH_TARGET_NODE) {
            return;
        }
        let Some(assoc) = r.operator_loc() else { return };
        let msg = format!("`{}` is overwritten by `rescue =>`.", node_text(&reference));
        let (kw_end, start, end) = (r.keyword_loc().end_offset(), assoc.start_offset(), assoc.end_offset());
        cx.add_fix(start, end, msg, |c| c.remove(kw_end, end));
    }
}
