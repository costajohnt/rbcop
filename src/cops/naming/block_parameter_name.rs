use crate::prelude::*;

pub const NAME: &str = "Naming/BlockParameterName";
pub static COP: BlockParameterName = BlockParameterName;
pub struct BlockParameterName;

impl Cop for BlockParameterName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let params = match node {
            Node::BlockNode { .. } => node.as_block_node().unwrap().parameters(),
            _ => node.as_lambda_node().unwrap().parameters(),
        };
        let Some(params) = params.and_then(|p| p.as_block_parameters_node()) else { return };
        let args = helpers::block_args::children(&params);
        helpers::uncommunicative_name::check(cx, &args, "block parameter");
    }
}
