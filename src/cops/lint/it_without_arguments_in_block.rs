use crate::prelude::*;

pub const NAME: &str = "Lint/ItWithoutArgumentsInBlock";
pub static COP: ItWithoutArgumentsInBlock = ItWithoutArgumentsInBlock;
pub struct ItWithoutArgumentsInBlock;

/// The parameters of the nearest parser `block` ancestor (numbered-parameter blocks are not
/// `block`s): `Some(None)` when it has none.
fn nearest_block_params<'pr>(ancestors: &[Node<'pr>]) -> Option<Option<Node<'pr>>> {
    for a in ancestors.iter().rev() {
        let params = match kind::of(a) {
            kind::LAMBDA_NODE => a.as_lambda_node().unwrap().parameters(),
            _ => {
                let block = match kind::of(a) {
                    kind::BLOCK_NODE => a.as_block_node(),
                    kind::CALL_NODE => a.as_call_node().unwrap().block().and_then(|b| b.as_block_node()),
                    kind::SUPER_NODE => a.as_super_node().unwrap().block().and_then(|b| b.as_block_node()),
                    kind::FORWARDING_SUPER_NODE => a.as_forwarding_super_node().unwrap().block(),
                    _ => None,
                };
                let Some(block) = block else { continue };
                block.parameters()
            }
        };
        if params.as_ref().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some()) {
            continue;
        }
        return Some(params);
    }
    None
}

impl Cop for ItWithoutArgumentsInBlock {
    fn ruby_range(&self) -> (f64, f64) {
        (0.0, 3.3)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "it" {
            return;
        }
        // `block_node.arguments.empty_and_without_delimiters?`
        let Some(None) = nearest_block_params(cx.ancestors) else { return };
        if call.receiver().is_some() || call.arguments().is_some() || call.opening_loc().is_some() || call.block().is_some() {
            return;
        }
        let loc = node.location();
        cx.add(
            loc.start_offset(),
            loc.end_offset(),
            "`it` calls without arguments will refer to the first block param in Ruby 3.4; use `it()` or `self.it`.",
        );
    }
}
