use crate::prelude::*;

pub const NAME: &str = "Style/EndBlock";
pub static COP: EndBlock = EndBlock;
pub struct EndBlock;

impl Cop for EndBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::POST_EXECUTION_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let kw = node.as_post_execution_node().unwrap().keyword_loc();
        let (s, e) = (kw.start_offset(), kw.end_offset());
        cx.add_fix(s, e, "Avoid the use of `END` blocks. Use `Kernel#at_exit` instead.", |c| c.replace(s, e, "at_exit"));
    }
}
