//! RuboCop's AllowedReceivers mixin.

use crate::ast::{is_csend, method_name, node_text};
use crate::cop::Cx;
use crate::node_kind as kind;
use ruby_prism::Node;

/// `receiver_name`: the receiver chain down to the first constant, as `a.b.c` (non-call
/// receivers by source).
pub fn receiver_name(node: &Node) -> String {
    let call = node.as_call_node();
    // rubocop-ast `receiver` of a block is its send's receiver; constants have none.
    if let Some(r) = call.as_ref().and_then(|c| c.receiver())
        && !matches!(kind::of(&r), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE)
    {
        return receiver_name(&r);
    }
    if let Some(call) = call.filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none())) {
        return match call.receiver() {
            Some(r) => format!("{}.{}", receiver_name(&r), method_name(&call)),
            None => method_name(&call).to_string(),
        };
    }
    node_text(node).to_string()
}

/// `allowed_receiver?(receiver)` against the cop's `AllowedReceivers`.
pub fn allowed_receiver(cx: &Cx, receiver: &Node) -> bool {
    cx.cfg.strings("AllowedReceivers").contains(&receiver_name(receiver))
}
