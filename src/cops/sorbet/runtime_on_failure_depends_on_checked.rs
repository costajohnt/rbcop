use crate::helpers::sorbet_signature::{each_call, signature};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/RuntimeOnFailureDependsOnChecked";
pub static COP: RuntimeOnFailureDependsOnChecked = RuntimeOnFailureDependsOnChecked;
pub struct RuntimeOnFailureDependsOnChecked;

impl Cop for RuntimeOnFailureDependsOnChecked {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if signature(node).is_none() {
            return;
        }
        let (mut on_failure, mut checked) = (false, false);
        each_call(node, |c| {
            if is_csend(c) {
                return;
            }
            match c.name().as_slice() {
                b"on_failure" => on_failure = true,
                b"checked" => {
                    let sym = |a: &Node| a.as_symbol_node().is_some_and(|s| matches!(s.unescaped(), b"tests" | b"always"));
                    checked |= matches!(send_args(c).as_slice(), [a] if sym(a));
                }
                _ => {}
            }
        });
        if on_failure && !checked {
            let (s, e) = helpers::node_types::range(node);
            cx.add(s, e, "To use .on_failure you must additionally call .checked(:tests) or .checked(:always), otherwise, the .on_failure has no effect.");
        }
    }
}
