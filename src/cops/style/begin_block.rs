use crate::prelude::*;

pub const NAME: &str = "Style/BeginBlock";
pub static COP: BeginBlock = BeginBlock;
pub struct BeginBlock;

impl Cop for BeginBlock {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PRE_EXECUTION_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let kw = node.as_pre_execution_node().unwrap().keyword_loc();
        cx.add(kw.start_offset(), kw.end_offset(), "Avoid the use of `BEGIN` blocks.");
    }
}
