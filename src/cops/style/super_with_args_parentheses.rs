use crate::prelude::*;

pub const NAME: &str = "Style/SuperWithArgsParentheses";
pub static COP: SuperWithArgsParentheses = SuperWithArgsParentheses;
pub struct SuperWithArgsParentheses;

impl Cop for SuperWithArgsParentheses {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::SUPER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let sup = node.as_super_node().unwrap();
        if sup.lparen_loc().is_some() {
            return;
        }
        // Parser `super` arguments: positional ones plus a `&blk` block pass.
        let mut args: Vec<(usize, usize)> = sup
            .arguments()
            .map(|a| a.arguments().iter().map(|n| (n.location().start_offset(), n.location().end_offset())).collect())
            .unwrap_or_default();
        if let Some(b) = sup.block().filter(|b| b.as_block_argument_node().is_some()) {
            args.push((b.location().start_offset(), b.location().end_offset()));
        }
        let (Some(&first), Some(&last)) = (args.first(), args.last()) else { return };
        let kw_end = sup.keyword_loc().end_offset();
        let start = node.location().start_offset();
        cx.add_fix(start, last.1, "Use parentheses for `super` with arguments.", |c| {
            c.replace(kw_end, first.0, "(");
            c.insert_after(last.0, last.1, ")");
        });
    }
}
