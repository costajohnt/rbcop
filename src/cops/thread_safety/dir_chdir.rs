use crate::helpers::node_types::plain_block_params;
use crate::helpers::parser_parent::{is_top_const, parser_parent};
use crate::prelude::*;

pub const NAME: &str = "ThreadSafety/DirChdir";
pub static COP: DirChdir = DirChdir;
pub struct DirChdir;

impl Cop for DirChdir {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call (const {nil? cbase} {:Dir :FileUtils}) :chdir ...) / (call (const {nil? cbase} :FileUtils) :cd ...)
        let call = node.as_call_node().unwrap();
        let Some(recv) = call.receiver() else { return };
        let module = match method_name(&call) {
            "chdir" if is_top_const(&recv, "Dir") => "Dir",
            "chdir" | "cd" if is_top_const(&recv, "FileUtils") => "FileUtils",
            _ => return,
        };
        // `node.block_argument? || node.parent&.block_type?`: a `&blk` argument, a `{ }`/`do` block
        // of its own (parser wraps the send), or the send is the body of a block.
        if cx.cfg.params.truthy("AllowCallWithBlock") {
            let own_block = call.block().is_some_and(|b| match b.as_block_node() {
                Some(b) => plain_block_params(b.parameters()),
                None => true,
            });
            let in_block = parser_parent(cx.ancestors).is_some_and(|p| match p {
                Node::BlockNode { .. } => plain_block_params(p.as_block_node().unwrap().parameters()),
                Node::LambdaNode { .. } => plain_block_params(p.as_lambda_node().unwrap().parameters()),
                _ => false,
            });
            if own_block || in_block {
                return;
            }
        }
        let dot = call.call_operator_loc().map_or("", |l| text(&l));
        let msg = format!("Avoid using `{module}{dot}{}` due to its process-wide effect.", method_name(&call));
        let (s, e) = send_range(&call);
        cx.add(s, e, msg);
    }
}
