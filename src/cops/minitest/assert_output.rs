use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertOutput";
pub static COP: AssertOutput = AssertOutput;
pub struct AssertOutput;

impl Cop for AssertOutput {
    fn kinds(&self) -> &'static [usize] {
        &[kind::GLOBAL_VARIABLE_WRITE_NODE, kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE, kind::GLOBAL_VARIABLE_OR_WRITE_NODE, kind::GLOBAL_VARIABLE_AND_WRITE_NODE, kind::GLOBAL_VARIABLE_TARGET_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let name = match node {
            Node::GlobalVariableWriteNode { .. } => node.as_global_variable_write_node().unwrap().name(),
            Node::GlobalVariableOperatorWriteNode { .. } => node.as_global_variable_operator_write_node().unwrap().name(),
            Node::GlobalVariableOrWriteNode { .. } => node.as_global_variable_or_write_node().unwrap().name(),
            Node::GlobalVariableAndWriteNode { .. } => node.as_global_variable_and_write_node().unwrap().name(),
            Node::GlobalVariableTargetNode { .. } => node.as_global_variable_target_node().unwrap().name(),
            _ => return,
        };
        let name = name.as_slice();
        if name != b"$stdout" && name != b"$stderr" {
            return;
        }
        let anc = cx.ancestors;
        // each_ancestor.find { test_case? }: the nearest def or test block.
        let Some(i) = (0..anc.len()).rev().find(|&i| test_case(&anc[i], &anc[..i])) else { return };
        let body = match &anc[i] {
            Node::DefNode { .. } => anc[i].as_def_node().unwrap().body(),
            _ => anc[i].as_call_node().unwrap().block().unwrap().as_block_node().unwrap().body(),
        };
        let gvar = String::from_utf8_lossy(name).into_owned();
        for assertion in body_assertions(body) {
            if references_gvar(&assertion.as_node(), name) {
                let (s, e) = send_range(&assertion);
                cx.add(s, e, format!("Use `assert_output` instead of mutating {gvar}."));
            }
        }
    }
}

/// `each_descendant(:gvar).any? { |d| d.children.first == name }`
fn references_gvar(node: &Node, name: &[u8]) -> bool {
    struct V<'a>(&'a [u8], bool);
    impl<'pr> ruby_prism::Visit<'pr> for V<'_> {
        fn visit_global_variable_read_node(&mut self, n: &ruby_prism::GlobalVariableReadNode<'pr>) {
            self.1 |= n.name().as_slice() == self.0;
        }
    }
    let mut v = V(name, false);
    ruby_prism::Visit::visit(&mut v, node);
    v.1
}
