use crate::prelude::*;

pub const NAME: &str = "Performance/FixedSize";
pub static COP: FixedSize = FixedSize;
pub struct FixedSize;

impl Cop for FixedSize {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "count" | "length" | "size") {
            return;
        }
        // allowed_parent?: a casgn or any block among the parser ancestors (its own block too,
        // and the block of a call it is an argument or receiver of).
        if call.block().is_some_and(|b| b.as_block_node().is_some()) || in_allowed_parent(cx.ancestors) {
            return;
        }
        // (call ${array hash str sym} {:count :length :size} $...)
        let Some(recv) = call.receiver() else { return };
        let target = cx.config.target_ruby;
        let ok = match kind::of(&recv) {
            kind::ARRAY_NODE => !recv.as_array_node().unwrap().elements().iter().any(|e| e.as_splat_node().is_some()),
            kind::HASH_NODE => {
                !recv.as_hash_node().unwrap().elements().iter().any(|e| e.as_assoc_splat_node().is_some())
            }
            kind::STRING_NODE | kind::SOURCE_FILE_NODE => !is_parser_dstr(&recv),
            kind::SYMBOL_NODE => !is_parser_dsym(&recv, target),
            _ => false,
        };
        if !ok {
            return;
        }
        // allowed_argument?: a first argument that is not a `str`.
        if send_args(&call).first().is_some_and(|a| {
            !(matches!(kind::of(a), kind::STRING_NODE | kind::SOURCE_FILE_NODE) && !is_parser_dstr(a))
        }) {
            return;
        }
        let (start, end) = send_range(&call);
        cx.add(start, end, "Do not compute the size of statically sized objects.");
    }
}

fn in_allowed_parent(anc: &[Node]) -> bool {
    anc.iter().any(|a| match kind::of(a) {
        kind::CONSTANT_WRITE_NODE | kind::CONSTANT_PATH_WRITE_NODE | kind::BLOCK_NODE | kind::LAMBDA_NODE => true,
        // A call with a block is inside its parser `block`.
        kind::CALL_NODE => a.as_call_node().unwrap().block().is_some_and(|b| b.as_block_node().is_some()),
        kind::SUPER_NODE => a.as_super_node().unwrap().block().is_some_and(|b| b.as_block_node().is_some()),
        kind::FORWARDING_SUPER_NODE => a.as_forwarding_super_node().unwrap().block().is_some(),
        _ => false,
    })
}
