//! RuboCop's CheckAssignment mixin (`on_*asgn` / `on_send` -> `check_assignment(node, rhs)`) and
//! `Util#first_part_of_call_chain`, on Prism nodes.

use crate::ast::send_args;
use crate::node_kind as kind;
use ruby_prism::Node;

/// Node kinds CheckAssignment hooks: every assignment plus calls (`on_send`).
pub const KINDS: &[usize] = &[
    kind::LOCAL_VARIABLE_WRITE_NODE,
    kind::INSTANCE_VARIABLE_WRITE_NODE,
    kind::CLASS_VARIABLE_WRITE_NODE,
    kind::GLOBAL_VARIABLE_WRITE_NODE,
    kind::CONSTANT_WRITE_NODE,
    kind::CONSTANT_PATH_WRITE_NODE,
    kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
    kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::CONSTANT_OPERATOR_WRITE_NODE,
    kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
    kind::LOCAL_VARIABLE_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_OR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
    kind::CONSTANT_OR_WRITE_NODE,
    kind::CONSTANT_PATH_OR_WRITE_NODE,
    kind::LOCAL_VARIABLE_AND_WRITE_NODE,
    kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
    kind::CLASS_VARIABLE_AND_WRITE_NODE,
    kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
    kind::CONSTANT_AND_WRITE_NODE,
    kind::CONSTANT_PATH_AND_WRITE_NODE,
    kind::CALL_OPERATOR_WRITE_NODE,
    kind::CALL_OR_WRITE_NODE,
    kind::CALL_AND_WRITE_NODE,
    kind::INDEX_OPERATOR_WRITE_NODE,
    kind::INDEX_OR_WRITE_NODE,
    kind::INDEX_AND_WRITE_NODE,
    kind::MULTI_WRITE_NODE,
    kind::CALL_NODE,
];

macro_rules! value_of {
    ($node:expr, $($k:ident => $as:ident),* $(,)?) => {
        match kind::of($node) {
            $(kind::$k => Some($node.$as().unwrap().value()),)*
            _ => None,
        }
    };
}

/// `extract_rhs`: an assignment's value, or a call's last argument (`&blk` included).
/// `csend` says whether `&.` calls count (cops that alias `on_csend`).
pub fn extract_rhs<'pr>(node: &Node<'pr>, csend: bool) -> Option<Node<'pr>> {
    if let Some(call) = node.as_call_node() {
        if !csend && crate::ast::is_csend(&call) {
            return None;
        }
        return send_args(&call).pop();
    }
    value_of!(node,
        LOCAL_VARIABLE_WRITE_NODE => as_local_variable_write_node,
        INSTANCE_VARIABLE_WRITE_NODE => as_instance_variable_write_node,
        CLASS_VARIABLE_WRITE_NODE => as_class_variable_write_node,
        GLOBAL_VARIABLE_WRITE_NODE => as_global_variable_write_node,
        CONSTANT_WRITE_NODE => as_constant_write_node,
        CONSTANT_PATH_WRITE_NODE => as_constant_path_write_node,
        LOCAL_VARIABLE_OPERATOR_WRITE_NODE => as_local_variable_operator_write_node,
        INSTANCE_VARIABLE_OPERATOR_WRITE_NODE => as_instance_variable_operator_write_node,
        CLASS_VARIABLE_OPERATOR_WRITE_NODE => as_class_variable_operator_write_node,
        GLOBAL_VARIABLE_OPERATOR_WRITE_NODE => as_global_variable_operator_write_node,
        CONSTANT_OPERATOR_WRITE_NODE => as_constant_operator_write_node,
        CONSTANT_PATH_OPERATOR_WRITE_NODE => as_constant_path_operator_write_node,
        LOCAL_VARIABLE_OR_WRITE_NODE => as_local_variable_or_write_node,
        INSTANCE_VARIABLE_OR_WRITE_NODE => as_instance_variable_or_write_node,
        CLASS_VARIABLE_OR_WRITE_NODE => as_class_variable_or_write_node,
        GLOBAL_VARIABLE_OR_WRITE_NODE => as_global_variable_or_write_node,
        CONSTANT_OR_WRITE_NODE => as_constant_or_write_node,
        CONSTANT_PATH_OR_WRITE_NODE => as_constant_path_or_write_node,
        LOCAL_VARIABLE_AND_WRITE_NODE => as_local_variable_and_write_node,
        INSTANCE_VARIABLE_AND_WRITE_NODE => as_instance_variable_and_write_node,
        CLASS_VARIABLE_AND_WRITE_NODE => as_class_variable_and_write_node,
        GLOBAL_VARIABLE_AND_WRITE_NODE => as_global_variable_and_write_node,
        CONSTANT_AND_WRITE_NODE => as_constant_and_write_node,
        CONSTANT_PATH_AND_WRITE_NODE => as_constant_path_and_write_node,
        CALL_OPERATOR_WRITE_NODE => as_call_operator_write_node,
        CALL_OR_WRITE_NODE => as_call_or_write_node,
        CALL_AND_WRITE_NODE => as_call_and_write_node,
        INDEX_OPERATOR_WRITE_NODE => as_index_operator_write_node,
        INDEX_OR_WRITE_NODE => as_index_or_write_node,
        INDEX_AND_WRITE_NODE => as_index_and_write_node,
        MULTI_WRITE_NODE => as_multi_write_node,
    )
}

/// `first_part_of_call_chain`: follow receivers (a call with a block is its send) to the start.
pub fn first_part_of_call_chain(node: Node) -> Option<Node> {
    let mut node = node;
    while let Some(call) = node.as_call_node() {
        node = call.receiver()?;
    }
    Some(node)
}
