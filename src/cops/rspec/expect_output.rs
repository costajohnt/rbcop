use crate::prelude::*;

pub const NAME: &str = "RSpec/ExpectOutput";
pub static COP: ExpectOutput = ExpectOutput;
pub struct ExpectOutput;

impl Cop for ExpectOutput {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::GLOBAL_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
            kind::GLOBAL_VARIABLE_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_gvasgn: `node.loc.name`
        let (name, (s, e)) = match node {
            Node::GlobalVariableWriteNode { .. } => {
                let n = node.as_global_variable_write_node().unwrap();
                (n.name(), (n.name_loc().start_offset(), n.name_loc().end_offset()))
            }
            Node::GlobalVariableOperatorWriteNode { .. } => {
                let n = node.as_global_variable_operator_write_node().unwrap();
                (n.name(), (n.name_loc().start_offset(), n.name_loc().end_offset()))
            }
            Node::GlobalVariableOrWriteNode { .. } => {
                let n = node.as_global_variable_or_write_node().unwrap();
                (n.name(), (n.name_loc().start_offset(), n.name_loc().end_offset()))
            }
            Node::GlobalVariableAndWriteNode { .. } => {
                let n = node.as_global_variable_and_write_node().unwrap();
                (n.name(), (n.name_loc().start_offset(), n.name_loc().end_offset()))
            }
            _ => (node.as_global_variable_target_node().unwrap().name(), helpers::rspec::node_range(node)),
        };
        let name = String::from_utf8_lossy(&name.as_slice()[1..]).into_owned();
        if name != "stdout" && name != "stderr" {
            return;
        }
        // inside_example_scope?: the nearest example group, example or hook decides
        let l = helpers::rspec::lang(cx);
        let mut inside = false;
        for a in cx.ancestors.iter().rev() {
            if helpers::rspec::example_group(&l, a) {
                break;
            }
            if helpers::rspec::example(&l, a) {
                inside = true;
                break;
            }
            if helpers::rspec::hook(&l, a) {
                inside = helpers::rspec::hook_scope(&a.as_call_node().unwrap()) == Some("each");
                break;
            }
        }
        if inside {
            cx.add(s, e, format!("Use `expect {{ ... }}.to output(...).to_{name}` instead of mutating ${name}."));
        }
    }
}
