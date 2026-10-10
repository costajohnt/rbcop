use crate::helpers::empty_parameter::empty_params;
use crate::prelude::*;

pub const NAME: &str = "Style/EmptyBlockParameter";
pub static COP: EmptyBlockParameter = EmptyBlockParameter;
pub struct EmptyBlockParameter;

impl Cop for EmptyBlockParameter {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let block = node.as_block_node().unwrap();
        let Some((start, end)) = empty_params(block.parameters()) else { return };
        let open = block.opening_loc().end_offset();
        cx.add_fix(start, end, "Omit pipes for the empty block parameters.", |c| c.remove(open, end));
    }
}
