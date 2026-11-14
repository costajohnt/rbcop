use crate::helpers::sorbet_rbs_assertion::{descendants, is_t_call, no_literal_block, t_send};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidUntypedStructProps";
pub static COP: ForbidUntypedStructProps = ForbidUntypedStructProps;
pub struct ForbidUntypedStructProps;

/// `{#t_untyped #t_nilable_untyped}`: `T.untyped`, or `T.nilable` of one of these.
fn untyped(node: &Node) -> bool {
    let Some(call) = node.as_call_node().filter(no_literal_block) else { return false };
    if is_t_call(&call, "untyped", Some(0)) {
        return true;
    }
    t_send(node, "nilable").is_some() && matches!(send_args(&call).as_slice(), [a] if untyped(a))
}

impl Cop for ForbidUntypedStructProps {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (class (const ...) (const (const nil? :T) {:Struct :ImmutableStruct}) ...)
        let class = node.as_class_node().unwrap();
        let t_struct = class.superclass().and_then(|s| s.as_constant_path_node()).is_some_and(|p| {
            p.name().is_some_and(|n| matches!(n.as_slice(), b"Struct" | b"ImmutableStruct"))
                && p.parent().is_some_and(|t| t.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"T"))
        });
        if !t_struct {
            return;
        }
        // def_node_search `(send nil? {:prop :const} _ ${#t_untyped #t_nilable_untyped} ...)`
        for n in descendants(node) {
            let Some(call) = n.as_call_node() else { continue };
            if call.receiver().is_some() || is_csend(&call) || !matches!(method_name(&call), "prop" | "const") {
                continue;
            }
            if let Some(t) = send_args(&call).get(1).filter(|a| untyped(a)) {
                let (s, e) = helpers::node_types::range(t);
                cx.add(s, e, "Struct props cannot be T.untyped");
            }
        }
    }
}
