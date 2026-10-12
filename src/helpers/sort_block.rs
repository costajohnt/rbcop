//! rubocop-performance's SortBlock mixin (RedundantSortBlock, SortReverse).

use crate::prelude::*;

/// `sort_with_block?` / `sort_with_numblock?` with `replaceable_body?`: a `sort` call (no
/// arguments) with a block `{ |a, b| x <=> y }` (or `{ _1 <=> _2 }`) whose body compares the
/// parameters in the given order (`reversed`: second first). Returns the sort call and the
/// sort_range end (the block's end).
pub fn sort_block<'pr>(node: &Node<'pr>, reversed: bool) -> Option<(ruby_prism::CallNode<'pr>, usize)> {
    let call = node.as_call_node()?;
    if method_name(&call) != "sort" || !call_args(&call).is_empty() {
        return None;
    }
    let block = call.block()?.as_block_node()?;
    let names: (Vec<u8>, Vec<u8>) = match block.parameters() {
        // (block $(call _ :sort) (args (arg $_a) (arg $_b)) $send)
        Some(p) if p.as_block_parameters_node().is_some() => {
            let bp = p.as_block_parameters_node().unwrap();
            if bp.locals().iter().count() != 0 {
                return None;
            }
            let ps = bp.parameters()?;
            let nodes = parameter_nodes(&ps);
            let [a, b] = nodes.as_slice() else { return None };
            let (a, b) = (a.as_required_parameter_node()?, b.as_required_parameter_node()?);
            (a.name().as_slice().to_vec(), b.name().as_slice().to_vec())
        }
        // (numblock $(call _ :sort) 2 $send)
        Some(p) if p.as_numbered_parameters_node().is_some_and(|n| n.maximum() == 2) => {
            (b"_1".to_vec(), b"_2".to_vec())
        }
        _ => return None,
    };
    let stmts = block.body()?.as_statements_node()?;
    let body: Vec<Node> = stmts.body().iter().collect();
    let [cmp] = body.as_slice() else { return None };
    // (send (lvar %1) :<=> (lvar %2))
    let cmp = cmp.as_call_node()?;
    if is_csend(&cmp) || method_name(&cmp) != "<=>" || cmp.block().is_some() {
        return None;
    }
    let lhs = cmp.receiver()?;
    let args = call_args(&cmp);
    let [rhs] = args.as_slice() else { return None };
    let (first, second) = if reversed { (&names.1, &names.0) } else { (&names.0, &names.1) };
    let lvar = |n: &Node, name: &[u8]| n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name);
    (lvar(&lhs, first) && lvar(rhs, second)).then(|| (call, block.closing_loc().end_offset()))
}
