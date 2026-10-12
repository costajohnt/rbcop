use crate::prelude::*;

pub const NAME: &str = "Rails/HelperInstanceVariable";
pub static COP: HelperInstanceVariable = HelperInstanceVariable;
pub struct HelperInstanceVariable;

const MSG: &str = "Do not use instance variables in helpers.";

impl Cop for HelperInstanceVariable {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::INSTANCE_VARIABLE_READ_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
            kind::INSTANCE_VARIABLE_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if cx.ancestors.iter().any(|a| a.as_class_node().is_some()) {
            return;
        }
        let (s, e) = match node {
            Node::InstanceVariableReadNode { .. } | Node::InstanceVariableTargetNode { .. } => {
                (node.location().start_offset(), node.location().end_offset())
            }
            Node::InstanceVariableWriteNode { .. } => {
                let l = node.as_instance_variable_write_node().unwrap().name_loc();
                (l.start_offset(), l.end_offset())
            }
            Node::InstanceVariableOperatorWriteNode { .. } => {
                let l = node.as_instance_variable_operator_write_node().unwrap().name_loc();
                (l.start_offset(), l.end_offset())
            }
            _ => {
                let l = node.as_instance_variable_and_write_node().unwrap().name_loc();
                (l.start_offset(), l.end_offset())
            }
        };
        cx.add(s, e, MSG);
    }
}
