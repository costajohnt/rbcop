use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceBeforeBrackets";
pub static COP: SpaceBeforeBrackets = SpaceBeforeBrackets;
pub struct SpaceBeforeBrackets;

const MSG: &str = "Remove the space before the opening brackets.";

impl Cop for SpaceBeforeBrackets {
    fn correctable(&self) -> bool {
        true
    }

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
        // (receiver, selector start, has dot): parser `send :[]` / `:[]=`, including the sends inside
        // index op-assigns and masgn targets.
        let (recv, sel, dot) = if let Some(c) = node.as_call_node() {
            if !matches!(method_name(&c), "[]" | "[]=") {
                return;
            }
            let (Some(r), Some(m)) = (c.receiver(), c.message_loc()) else { return };
            (r, m.start_offset(), c.call_operator_loc().is_some())
        } else if let Some(n) = node.as_index_operator_write_node() {
            let Some(r) = n.receiver() else { return };
            (r, n.opening_loc().start_offset(), n.call_operator_loc().is_some())
        } else if let Some(n) = node.as_index_or_write_node() {
            let Some(r) = n.receiver() else { return };
            (r, n.opening_loc().start_offset(), n.call_operator_loc().is_some())
        } else if let Some(n) = node.as_index_and_write_node() {
            let Some(r) = n.receiver() else { return };
            (r, n.opening_loc().start_offset(), n.call_operator_loc().is_some())
        } else {
            let n = node.as_index_target_node().unwrap();
            (n.receiver(), n.opening_loc().start_offset(), false)
        };
        if dot {
            return;
        }
        let recv_end = recv.location().end_offset();
        if recv_end >= sel {
            return;
        }
        cx.add_fix(recv_end, sel, MSG, |c| c.remove(recv_end, sel));
    }
}
