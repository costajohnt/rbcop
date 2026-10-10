use crate::prelude::*;

pub const NAME: &str = "Lint/HashCompareByIdentity";
pub static COP: HashCompareByIdentity = HashCompareByIdentity;
pub struct HashCompareByIdentity;

const MSG: &str = "Use `Hash#compare_by_identity` instead of using `object_id` for keys.";

/// `(send _ :object_id)` (not csend).
fn object_id_send(arg: Option<Node>) -> bool {
    arg.and_then(|a| a.as_call_node()).is_some_and(|c| !is_csend(&c) && method_name(&c) == "object_id")
}

impl Cop for HashCompareByIdentity {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
            kind::INDEX_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // parser's op_asgn / or_asgn / and_asgn / mlhs hold an `[]` send that on_send sees.
        macro_rules! index {
            ($n:expr) => {{
                let n = $n;
                if object_id_send(n.arguments().and_then(|a| a.arguments().iter().next())) {
                    let start = n.location().start_offset();
                    cx.add(start, n.closing_loc().end_offset(), MSG);
                }
                return;
            }};
        }
        if let Some(n) = node.as_index_operator_write_node() {
            index!(n);
        }
        if let Some(n) = node.as_index_or_write_node() {
            index!(n);
        }
        if let Some(n) = node.as_index_and_write_node() {
            index!(n);
        }
        if let Some(n) = node.as_index_target_node() {
            index!(n);
        }
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "key?" | "has_key?" | "fetch" | "[]" | "[]=") {
            return;
        }
        if object_id_send(call_args(&call).into_iter().next()) {
            let (s, e) = send_range(&call);
            cx.add(s, e, MSG);
        }
    }
}
