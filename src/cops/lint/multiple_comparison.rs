use crate::prelude::*;

pub const NAME: &str = "Lint/MultipleComparison";
pub static COP: MultipleComparison = MultipleComparison;
pub struct MultipleComparison;

const COMPARISON: &[&str] = &["<", ">", "<=", ">="];

impl Cop for MultipleComparison {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send _ {:< :> :<= :>=} $_) {:< :> :<= :>=} _)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !COMPARISON.contains(&method_name(&call)) || send_args(&call).len() != 1 {
            return;
        }
        let Some(inner) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if is_csend(&inner) || inner.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let inner_args = send_args(&inner);
        if !COMPARISON.contains(&method_name(&inner)) || inner_args.len() != 1 {
            return;
        }
        let center = &inner_args[0];
        if center.as_call_node().is_some_and(|c| !is_csend(&c) && ["&", "|", "^"].contains(&method_name(&c))) {
            return;
        }
        let loc = center.location();
        let (cs, ce) = (loc.start_offset(), loc.end_offset());
        let text = node_text(center);
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, "Use the `&&` operator to compare multiple values.", |c| {
            c.replace(cs, ce, &format!("{text} && {text}"));
        });
    }
}
