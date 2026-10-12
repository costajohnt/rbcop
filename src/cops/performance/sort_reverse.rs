use crate::prelude::*;

pub const NAME: &str = "Performance/SortReverse";
pub static COP: SortReverse = SortReverse;
pub struct SortReverse;

impl Cop for SortReverse {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((sort, end)) = helpers::sort_block::sort_block(node, true) else { return };
        // `send.loc.dot.source` on a receiver-less sort raises: no offense.
        let Some(dot) = sort.call_operator_loc() else { return };
        let prefer = format!("sort{}reverse", text(&dot));
        let start = sort.message_loc().unwrap().start_offset();
        cx.add_fix(start, end, format!("Use `{prefer}` instead."), |c| c.replace(start, end, &prefer));
    }
}
