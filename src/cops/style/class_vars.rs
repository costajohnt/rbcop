use crate::prelude::*;

pub const NAME: &str = "Style/ClassVars";
pub static COP: ClassVars = ClassVars;
pub struct ClassVars;

fn msg(var: &str) -> String {
    format!("Replace class var {var} with a class instance var.")
}

impl Cop for ClassVars {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::CLASS_VARIABLE_WRITE_NODE,
            kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CLASS_VARIABLE_OR_WRITE_NODE,
            kind::CLASS_VARIABLE_AND_WRITE_NODE,
            kind::CLASS_VARIABLE_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // Every cvasgn (also inside op_asgn / or_asgn / and_asgn / masgn / rescue targets).
        let name_loc = match node {
            Node::ClassVariableWriteNode { .. } => Some(node.as_class_variable_write_node().unwrap().name_loc()),
            Node::ClassVariableOperatorWriteNode { .. } => {
                Some(node.as_class_variable_operator_write_node().unwrap().name_loc())
            }
            Node::ClassVariableOrWriteNode { .. } => Some(node.as_class_variable_or_write_node().unwrap().name_loc()),
            Node::ClassVariableAndWriteNode { .. } => Some(node.as_class_variable_and_write_node().unwrap().name_loc()),
            Node::ClassVariableTargetNode { .. } => Some(node.location()),
            _ => None,
        };
        if let Some(loc) = name_loc {
            cx.add(loc.start_offset(), loc.end_offset(), msg(text(&loc)));
            return;
        }
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "class_variable_set" {
            return;
        }
        if let Some(arg) = send_args(&call).into_iter().next() {
            let loc = arg.location();
            cx.add(loc.start_offset(), loc.end_offset(), msg(node_text(&arg)));
        }
    }
}
