use crate::prelude::*;
use ruby_prism::{BlockNode, CallNode};

pub const NAME: &str = "Style/CombinableLoops";
pub static COP: CombinableLoops = CombinableLoops;
pub struct CombinableLoops;

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// A statement that is a parser `block`/`numblock`/`itblock`: a call with a literal block.
fn as_block<'pr>(n: &Node<'pr>) -> Option<(CallNode<'pr>, BlockNode<'pr>)> {
    let call = n.as_call_node()?;
    let block = call.block()?.as_block_node()?;
    Some((call, block))
}

/// Parser-node `==` of two optional nodes.
fn same_opt(src: &crate::source::Source, a: Option<Node>, b: Option<Node>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => helpers::node_equality::same(src, range(&a), range(&b)),
        _ => false,
    }
}

/// Block `arguments` as compared by `==`: None for `[]` (numblock/itblock), Some(range) for an
/// `args` node (empty range when there are no parameters).
fn block_args(b: &BlockNode) -> Option<Option<(usize, usize)>> {
    match b.parameters() {
        None => Some(None),
        Some(p) => p.as_block_parameters_node().map(|bp| match bp.parameters() {
                None if bp.locals().iter().next().is_none() => None,
                _ => Some(range(&p)),
            }),
    }
}

impl Cop for CombinableLoops {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let stmts: Vec<Node> = node.as_statements_node().unwrap().body().iter().collect();
        if stmts.len() < 2 {
            return;
        }
        // `begin a; b; end` without rescue/ensure is `(kwbegin a b)`: no parser `begin` parent.
        if let Some(b) = cx.ancestors.last().and_then(|a| a.as_begin_node())
            && b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none() {
                return;
            }
        let src = cx.src;
        for i in 1..stmts.len() {
            let (left, cur) = (&stmts[i - 1], &stmts[i]);
            if let (Some((lc, lb)), Some((cc, cb))) = (as_block(left), as_block(cur)) {
                let name = method_name(&cc);
                if !(name.starts_with("each") || name.ends_with("_each")) || method_name(&lc) != name {
                    continue;
                }
                if !same_opt(src, lc.receiver(), cc.receiver()) {
                    continue;
                }
                let (la, ca) = (call_args(&lc), call_args(&cc));
                if la.len() != ca.len() || la.iter().zip(&ca).any(|(a, b)| !helpers::node_equality::same(src, range(a), range(b))) {
                    continue;
                }
                let (Some(lbody), Some(cbody)) = (lb.body(), cb.body()) else { continue };
                let same_args = match (block_args(&lb), block_args(&cb)) {
                    (Some(None), Some(None)) | (None, None) => true,
                    (Some(Some(a)), Some(Some(b))) => helpers::node_equality::same(src, a, b),
                    _ => false,
                };
                let lbody_end = body_range(&lbody).1;
                let l_end = lb.closing_loc().end_offset();
                let cbody_start = body_range(&cbody).0;
                let (cs, ce) = range(cur);
                let c_close = (cb.closing_loc().start_offset(), cb.closing_loc().end_offset());
                let right_is_block = stmts.get(i + 1).is_some_and(|r| as_block(r).is_some() || r.as_lambda_node().is_some());
                let end_of_block = if lb.opening_loc().as_slice() == b"{" { "}" } else { " end" };
                cx.add_fix(cs, ce, "Combine this loop with the previous loop.", |c| {
                    if !same_args {
                        return;
                    }
                    c.remove(lbody_end, l_end);
                    c.remove(cs, cbody_start);
                    if !right_is_block {
                        c.remove(c_close.0, c_close.1);
                        c.insert_before(ce, ce, end_of_block);
                    }
                });
            } else if let (Some(lf), Some(cf)) = (left.as_for_node(), cur.as_for_node()) {
                if !helpers::node_equality::same(src, range(&lf.collection()), range(&cf.collection())) {
                    continue;
                }
                let (Some(lbody), Some(cbody)) = (lf.statements(), cf.statements()) else { continue };
                let same_var = helpers::node_equality::same(src, range(&lf.index()), range(&cf.index()));
                let lbody_end = lbody.location().end_offset();
                let l_end = lf.end_keyword_loc().end_offset();
                let cbody_start = cbody.location().start_offset();
                let (cs, ce) = range(cur);
                cx.add_fix(cs, ce, "Combine this loop with the previous loop.", |c| {
                    if same_var {
                        c.remove(lbody_end, l_end);
                        c.remove(cs, cbody_start);
                    }
                });
            }
        }
    }
}
