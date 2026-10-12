use crate::prelude::*;

pub const NAME: &str = "Rails/RedundantTravelBack";
pub static COP: RedundantTravelBack = RedundantTravelBack;
pub struct RedundantTravelBack;

impl Cop for RedundantTravelBack {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.2
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "travel_back" {
            return;
        }
        // each_ancestor(:def, :block).any? { teardown def / after block }
        let anc = cx.ancestors;
        let inside = anc.iter().enumerate().any(|(i, a)| {
            if let Some(d) = a.as_def_node() {
                return d.receiver().is_none() && d.name().as_slice() == b"teardown";
            }
            a.as_block_node().is_some()
                && i > 0
                && helpers::rails::block_call(&anc[i - 1]).is_some_and(|c| method_name(&c) == "after")
        });
        if !inside {
            return;
        }
        let (s, e) = send_range(&call);
        let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true);
        cx.add_fix(s, e, "Redundant `travel_back` detected.", |c| c.remove(rs, re));
    }
}
