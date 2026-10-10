//! rubocop-ast node-type predicates over Prism nodes.

use crate::prelude::*;

/// rubocop-ast `assignment?`: (lvasgn ivasgn cvasgn gvasgn casgn masgn op_asgn or_asgn and_asgn),
/// plus `setter_method?` (its alias on send nodes: `a.b = x`, `a[i] = x`).
pub fn is_assignment(node: &Node) -> bool {
    if let Some(c) = node.as_call_node() {
        return c.equal_loc().is_some();
    }
    ASSIGNMENTS.contains(&kind::of(node))
}

const ASSIGNMENTS: &[usize] = &[
    kind::CALL_AND_WRITE_NODE,
    kind::CALL_OPERATOR_WRITE_NODE,
    kind::CALL_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_AND_WRITE_NODE,
    kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
    kind::CLASS_VARIABLE_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_WRITE_NODE,
    kind::CONSTANT_AND_WRITE_NODE,
    kind::CONSTANT_OPERATOR_WRITE_NODE,
    kind::CONSTANT_OR_WRITE_NODE,
    kind::CONSTANT_PATH_AND_WRITE_NODE,
    kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
    kind::CONSTANT_PATH_OR_WRITE_NODE,
    kind::CONSTANT_PATH_WRITE_NODE,
    kind::CONSTANT_WRITE_NODE,
    kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_WRITE_NODE,
    kind::INDEX_AND_WRITE_NODE,
    kind::INDEX_OPERATOR_WRITE_NODE,
    kind::INDEX_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_WRITE_NODE,
    kind::LOCAL_VARIABLE_AND_WRITE_NODE,
    kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::LOCAL_VARIABLE_OR_WRITE_NODE,
    kind::LOCAL_VARIABLE_WRITE_NODE,
    kind::MULTI_WRITE_NODE,
];

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

/// A `_1`..`_9` read below Ruby 2.7: Prism's LocalVariableReadNode is the parser gem's
/// `(send nil :_1)` there (the runner already dropped the blocks' numbered parameters).
pub fn is_legacy_numbered_read(cx: &Cx, node: &Node) -> bool {
    cx.config.target_ruby < 2.7
        && node.as_local_variable_read_node().is_some_and(|l| {
            matches!(l.name().as_slice(), [b'_', b'1'..=b'9'])
        })
}

/// `operator_method?`
pub fn is_operator_method(name: &str) -> bool {
    OPERATOR_METHODS.contains(&name)
}

/// `binary_operation?`: an operator method whose selector does not start the expression
/// (`a + b`, `a[1]`, `a.+(b)`).
pub fn is_binary_operation(call: &ruby_prism::CallNode) -> bool {
    call.message_loc().is_some_and(|sel| {
        is_operator_method(method_name(call)) && sel.start_offset() != call.location().start_offset()
    })
}

/// `unary_operation?`: an operator method whose selector starts the expression (`-a`, `!a`, `not a`).
pub fn is_unary_operation(call: &ruby_prism::CallNode) -> bool {
    call.message_loc().is_some_and(|sel| {
        is_operator_method(method_name(call)) && sel.start_offset() == call.location().start_offset()
    })
}
