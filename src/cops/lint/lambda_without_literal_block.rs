use crate::prelude::*;

pub const NAME: &str = "Lint/LambdaWithoutLiteralBlock";
pub static COP: LambdaWithoutLiteralBlock = LambdaWithoutLiteralBlock;
pub struct LambdaWithoutLiteralBlock;

/// Parser `block` (not numblock/itblock): a BlockNode or lambda literal without numbered/`it` params.
fn is_parser_block(node: &Node) -> bool {
    let params = match (node.as_block_node(), node.as_lambda_node()) {
        (Some(b), _) => b.parameters(),
        (_, Some(l)) => l.parameters(),
        _ => return false,
    };
    !params.is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}

impl Cop for LambdaWithoutLiteralBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "lambda" {
            return;
        }
        // node.parent&.block_type?: a literal block on this call, or this call as a block's body.
        if call.block().is_some_and(|b| is_parser_block(&b)) {
            return;
        }
        if helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(is_parser_block) {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        // (send nil? :lambda (block_pass (sym _)))
        let symbol_proc = call.receiver().is_none()
            && args.len() == 1
            && first.as_block_argument_node().and_then(|b| b.expression()).is_some_and(|e| e.as_symbol_node().is_some());
        if symbol_proc {
            return;
        }
        let replacement = node_text(first).strip_prefix('&').unwrap_or(node_text(first)).to_string();
        let (start, end) = send_range(&call);
        cx.add_fix(
            start,
            end,
            "lambda without a literal block is deprecated; use the proc without lambda instead.",
            |c| c.replace(start, end, &replacement),
        );
    }
}
