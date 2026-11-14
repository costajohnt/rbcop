use crate::helpers::sorbet_signature::is_t_const;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/RedundantExtendTSig";
pub static COP: RedundantExtendTSig = RedundantExtendTSig;
pub struct RedundantExtendTSig;

impl Cop for RedundantExtendTSig {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || call.name().as_slice() != b"extend" {
            return;
        }
        if !matches!(send_args(&call).as_slice(), [a] if is_t_const(a, "Sig")) {
            return;
        }
        let (s, e) = send_range(&call);
        let (ls, le) = helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true);
        cx.add_fix(s, e, "Do not redundantly `extend T::Sig` when it is already included in all modules.", |c| {
            c.remove(ls, le)
        });
    }
}
