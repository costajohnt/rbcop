use crate::prelude::*;
use ruby_prism::Location;

pub const NAME: &str = "Style/ItAssignment";
pub static COP: ItAssignment = ItAssignment;
pub struct ItAssignment;

impl Cop for ItAssignment {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::LOCAL_VARIABLE_TARGET_NODE,
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::LOCAL_VARIABLE_OR_WRITE_NODE,
            kind::LOCAL_VARIABLE_AND_WRITE_NODE,
            kind::REQUIRED_PARAMETER_NODE,
            kind::OPTIONAL_PARAMETER_NODE,
            kind::REST_PARAMETER_NODE,
            kind::BLOCK_PARAMETER_NODE,
            kind::REQUIRED_KEYWORD_PARAMETER_NODE,
            kind::OPTIONAL_KEYWORD_PARAMETER_NODE,
            kind::KEYWORD_REST_PARAMETER_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let name: Option<Location> = match node {
            Node::LocalVariableWriteNode { .. } => Some(node.as_local_variable_write_node().unwrap().name_loc()),
            Node::LocalVariableTargetNode { .. } => {
                // Only masgn / rescue / for targets are parser `lvasgn`: pattern variables are
                // `match_var`, regexp captures part of `match_with_lvasgn`.
                let lvasgn = cx.parent().is_some_and(|p| {
                    matches!(
                        p,
                        Node::MultiWriteNode { .. }
                            | Node::MultiTargetNode { .. }
                            | Node::RescueNode { .. }
                            | Node::ForNode { .. }
                    )
                });
                lvasgn.then(|| node.location())
            }
            Node::LocalVariableOperatorWriteNode { .. } => {
                Some(node.as_local_variable_operator_write_node().unwrap().name_loc())
            }
            Node::LocalVariableOrWriteNode { .. } => Some(node.as_local_variable_or_write_node().unwrap().name_loc()),
            Node::LocalVariableAndWriteNode { .. } => Some(node.as_local_variable_and_write_node().unwrap().name_loc()),
            Node::RequiredParameterNode { .. } => Some(node.location()),
            Node::OptionalParameterNode { .. } => Some(node.as_optional_parameter_node().unwrap().name_loc()),
            Node::RestParameterNode { .. } => node.as_rest_parameter_node().unwrap().name_loc(),
            Node::BlockParameterNode { .. } => node.as_block_parameter_node().unwrap().name_loc(),
            Node::RequiredKeywordParameterNode { .. } => {
                Some(node.as_required_keyword_parameter_node().unwrap().name_loc())
            }
            Node::OptionalKeywordParameterNode { .. } => {
                Some(node.as_optional_keyword_parameter_node().unwrap().name_loc())
            }
            _ => node.as_keyword_rest_parameter_node().unwrap().name_loc(),
        };
        let Some(loc) = name else { return };
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let text = &cx.src.text[start..end];
        if text.trim_end_matches(':') != "it" {
            return;
        }
        cx.add(start, start + 2, "`it` is the default block parameter; consider another name.");
    }
}
