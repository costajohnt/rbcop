use crate::prelude::*;
use std::collections::HashSet;

pub const NAME: &str = "Style/YodaExpression";
pub static COP: YodaExpression = YodaExpression;
pub struct YodaExpression;

/// `node.type?(:numeric, :const)`
fn constant_portion(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::INTEGER_NODE
            | kind::FLOAT_NODE
            | kind::RATIONAL_NODE
            | kind::IMAGINARY_NODE
            | kind::CONSTANT_READ_NODE
            | kind::CONSTANT_PATH_NODE
    )
}

impl Cop for YodaExpression {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "*" | "+" | "&" | "|" | "^") {
            return;
        }
        if !cx.cfg.strings("SupportedOperators").iter().any(|o| o == name) {
            return;
        }
        let args = send_args(&call);
        let (Some(lhs), Some(rhs)) = (call.receiver(), args.first()) else { return };
        if !constant_portion(&lhs) || constant_portion(rhs) {
            return;
        }
        // `each_ancestor(:send)`: a call owning the block we are in is not a parser ancestor.
        let anc = cx.ancestors;
        let ancestor_ranges: Vec<(usize, usize)> = (0..anc.len())
            .filter_map(|i| {
                let c = anc[i].as_call_node()?;
                let owns_block = anc.get(i + 1).is_some_and(|n| kind::of(n) == kind::BLOCK_NODE);
                (!is_csend(&c) && !owns_block).then(|| send_range(&c))
            })
            .collect();
        let offended = cx.state::<HashSet<(usize, usize)>>();
        if ancestor_ranges.iter().any(|r| offended.contains(r)) {
            return;
        }
        let range = send_range(&call);
        let msg = format!("Non-literal operand (`{}`) should be first.", node_text(rhs));
        let (l, r) = (lhs.location(), rhs.location());
        let (l, r) = ((l.start_offset(), l.end_offset()), (r.start_offset(), r.end_offset()));
        cx.add_fix(range.0, range.1, msg, |c| c.swap(l, r));
        cx.state::<HashSet<(usize, usize)>>().insert(range);
    }
}
