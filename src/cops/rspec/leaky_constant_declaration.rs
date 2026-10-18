use crate::prelude::*;

pub const NAME: &str = "RSpec/LeakyConstantDeclaration";
pub static COP: LeakyConstantDeclaration = LeakyConstantDeclaration;
pub struct LeakyConstantDeclaration;

impl Cop for LeakyConstantDeclaration {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_OPERATOR_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_AND_WRITE_NODE,
            kind::CONSTANT_TARGET_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
            kind::CONSTANT_PATH_AND_WRITE_NODE,
            kind::CONSTANT_PATH_TARGET_NODE,
            kind::CLASS_NODE,
            kind::MODULE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // inside_describe_block?: a plain-block ancestor that is a spec group
        let l = helpers::rspec::lang(cx);
        if !cx.ancestors.iter().rev().any(|a| helpers::rspec::plain_block_call(a).is_some() && helpers::rspec::spec_group(&l, a)) {
            return;
        }
        // Only an unqualified name (no namespace, not even `::`) is reported: a `casgn` with a
        // namespace (`::A = 1`, `B::A ||= 1`) is asked inside_describe_block? and no more.
        if matches!(
            kind::of(node),
            kind::CONSTANT_PATH_WRITE_NODE
                | kind::CONSTANT_PATH_OPERATOR_WRITE_NODE
                | kind::CONSTANT_PATH_OR_WRITE_NODE
                | kind::CONSTANT_PATH_AND_WRITE_NODE
                | kind::CONSTANT_PATH_TARGET_NODE
        ) {
            return;
        }
        let path = |p: Node| p.as_constant_read_node().is_some();
        let ((s, e), msg) = match node {
            Node::ClassNode { .. } => {
                if !path(node.as_class_node().unwrap().constant_path()) {
                    return;
                }
                (helpers::rspec::node_range(node), "Stub class constant instead of declaring explicitly.")
            }
            Node::ModuleNode { .. } => {
                if !path(node.as_module_node().unwrap().constant_path()) {
                    return;
                }
                (helpers::rspec::node_range(node), "Stub module constant instead of declaring explicitly.")
            }
            Node::ConstantWriteNode { .. } => (helpers::rspec::node_range(node), "Stub constant instead of declaring explicitly."),
            Node::ConstantOperatorWriteNode { .. } => {
                let l = node.as_constant_operator_write_node().unwrap().name_loc();
                ((l.start_offset(), l.end_offset()), "Stub constant instead of declaring explicitly.")
            }
            Node::ConstantOrWriteNode { .. } => {
                let l = node.as_constant_or_write_node().unwrap().name_loc();
                ((l.start_offset(), l.end_offset()), "Stub constant instead of declaring explicitly.")
            }
            Node::ConstantAndWriteNode { .. } => {
                let l = node.as_constant_and_write_node().unwrap().name_loc();
                ((l.start_offset(), l.end_offset()), "Stub constant instead of declaring explicitly.")
            }
            _ => (helpers::rspec::node_range(node), "Stub constant instead of declaring explicitly."),
        };
        cx.add(s, e, msg);
    }
}
