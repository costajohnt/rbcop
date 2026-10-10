use crate::prelude::*;

pub const NAME: &str = "Layout/SingleLineBlockChain";
pub static COP: SingleLineBlockChain = SingleLineBlockChain;
pub struct SingleLineBlockChain;

const MSG: &str = "Put method call on a separate line if chained to a single line block.";

/// `loc.begin` / `loc.end` of an `any_block` receiver.
fn block_delimiters(n: &Node) -> Option<(usize, usize)> {
    let block = match n {
        Node::LambdaNode { .. } => {
            let l = n.as_lambda_node().unwrap();
            return Some((l.opening_loc().start_offset(), l.closing_loc().start_offset()));
        }
        Node::CallNode { .. } => n.as_call_node().unwrap().block(),
        Node::SuperNode { .. } => n.as_super_node().unwrap().block(),
        Node::ForwardingSuperNode { .. } => n.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        _ => None,
    }?;
    let b = block.as_block_node()?;
    Some((b.opening_loc().start_offset(), b.closing_loc().start_offset()))
}

impl Cop for SingleLineBlockChain {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some((open, close)) = call.receiver().and_then(|r| block_delimiters(&r)) else { return };
        let close_line = cx.src.line_of(close);
        if cx.src.line_of(open) < close_line {
            return;
        }
        let Some(dot) = call.call_operator_loc() else { return };
        // `loc.selector || loc.begin`
        let Some(sel) = call.message_loc().or_else(|| call.opening_loc()) else { return };
        if cx.src.line_of(dot.start_offset()) > close_line
            || cx.src.column(dot.start_offset()) >= cx.src.column(sel.start_offset())
        {
            return;
        }
        let (s, e) = (dot.start_offset(), sel.end_offset());
        cx.add_fix(s, e, MSG, |c| c.insert_before(s, e, "\n"));
    }
}
