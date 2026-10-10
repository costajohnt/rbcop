use crate::prelude::*;

pub const NAME: &str = "Style/MethodCalledOnDoEndBlock";
pub static COP: MethodCalledOnDoEndBlock = MethodCalledOnDoEndBlock;
pub struct MethodCalledOnDoEndBlock;

/// `loc.end` of a `do ... end` block (any_block with `keywords?`), if `n` is one.
fn do_end_block_end(n: &Node) -> Option<usize> {
    let block = match n {
        Node::CallNode { .. } => n.as_call_node().unwrap().block(),
        Node::SuperNode { .. } => n.as_super_node().unwrap().block(),
        Node::ForwardingSuperNode { .. } => n.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        Node::LambdaNode { .. } => {
            let l = n.as_lambda_node().unwrap();
            return (l.opening_loc().as_slice() == b"do").then(|| l.closing_loc().start_offset());
        }
        _ => None,
    }?;
    let b = block.as_block_node()?;
    (b.opening_loc().as_slice() == b"do").then(|| b.closing_loc().start_offset())
}

impl Cop for MethodCalledOnDoEndBlock {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // on_block ignores the block's own send
        if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let Some(start) = call.receiver().and_then(|r| do_end_block_end(&r)) else { return };
        cx.add(start, send_range(&call).1, "Avoid chaining a method call on a do...end block.");
    }
}
