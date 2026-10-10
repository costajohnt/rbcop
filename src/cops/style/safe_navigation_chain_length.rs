use crate::helpers::parser_path::{Tag, frames};
use crate::prelude::*;

pub const NAME: &str = "Style/SafeNavigationChainLength";
pub static COP: SafeNavigationChainLength = SafeNavigationChainLength;
pub struct SafeNavigationChainLength;

impl Cop for SafeNavigationChainLength {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // A csend with a block has the block as its parser parent.
        if !is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let max = cx.cfg.int("Max").unwrap_or(2);
        let chain: Vec<&Node> = cx.ancestors.iter().collect();
        let parents = frames(&chain, node);
        let csends: Vec<_> = parents.iter().rev().take_while(|f| f.tag == Tag::Csend).collect();
        if (csends.len() as i64) < max {
            return;
        }
        let Some(last) = csends.last() else { return };
        let (start, end) = last.range;
        cx.add(start, end, format!("Avoid safe navigation chains longer than {max} calls."));
    }
}
