use crate::prelude::*;

pub const NAME: &str = "Performance/RedundantSortBlock";
pub static COP: RedundantSortBlock = RedundantSortBlock;
pub struct RedundantSortBlock;

impl Cop for RedundantSortBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((sort, end)) = helpers::sort_block::sort_block(node, false) else { return };
        let start = sort.message_loc().unwrap().start_offset();
        cx.add_fix(start, end, "Use `sort` without block.", |c| c.replace(start, end, "sort"));
    }
}
