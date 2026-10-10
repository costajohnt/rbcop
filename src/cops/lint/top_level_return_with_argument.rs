use crate::prelude::*;

pub const NAME: &str = "Lint/TopLevelReturnWithArgument";
pub static COP: TopLevelReturnWithArgument = TopLevelReturnWithArgument;
pub struct TopLevelReturnWithArgument;

impl Cop for TopLevelReturnWithArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RETURN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if node.as_return_node().unwrap().arguments().is_none() {
            return;
        }
        if cx.ancestors.iter().any(|a| matches!(kind::of(a), kind::DEF_NODE | kind::BLOCK_NODE | kind::LAMBDA_NODE)) {
            return;
        }
        let l = node.location();
        let (s, e) = (l.start_offset(), l.end_offset());
        cx.add_fix(s, e, "Top level return with argument detected.", |c| c.replace(s, e, "return"));
    }
}
