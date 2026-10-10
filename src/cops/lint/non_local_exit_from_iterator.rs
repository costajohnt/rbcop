use crate::prelude::*;

pub const NAME: &str = "Lint/NonLocalExitFromIterator";
pub static COP: NonLocalExitFromIterator = NonLocalExitFromIterator;
pub struct NonLocalExitFromIterator;

const MSG: &str = "Non-local exit from iterator, without return value. \
                   `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.";

/// rubocop-ast `BlockNode#argument_list` is empty.
fn no_arguments(block: &ruby_prism::BlockNode) -> bool {
    let Some(params) = block.parameters() else { return true };
    let Some(bp) = params.as_block_parameters_node() else { return false };
    bp.parameters().is_none() && bp.locals().iter().next().is_none()
}

impl Cop for NonLocalExitFromIterator {
    fn kinds(&self) -> &'static [usize] {
        &[kind::RETURN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let ret = node.as_return_node().unwrap();
        if ret.arguments().is_some() {
            return;
        }
        // each_ancestor(:any_block, :any_def): a call owning a block stands for the parser block
        // whether we sit in its block or its receiver/arguments.
        for anc in cx.ancestors.iter().rev() {
            if anc.as_def_node().is_some() || anc.as_lambda_node().is_some() {
                return;
            }
            let Some(call) = anc.as_call_node() else { continue };
            let Some(block) = call.block().and_then(|b| b.as_block_node()) else { continue };
            let name = method_name(&call);
            if name == "lambda" {
                return; // BlockNode#lambda?
            }
            // (send _ {:define_method :define_singleton_method} _)
            if !is_csend(&call)
                && matches!(name, "define_method" | "define_singleton_method")
                && call_args(&call).len() == 1
            {
                return;
            }
            if no_arguments(&block) {
                continue;
            }
            if call.receiver().is_some() {
                let kw = ret.keyword_loc();
                cx.add(kw.start_offset(), kw.end_offset(), MSG);
                return;
            }
        }
    }
}
