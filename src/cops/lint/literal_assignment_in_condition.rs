use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/LiteralAssignmentInCondition";
pub static COP: LiteralAssignmentInCondition = LiteralAssignmentInCondition;
pub struct LiteralAssignmentInCondition;

/// `all_literals?`
fn all_literals(node: &Node) -> bool {
    match node {
        Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().iter().all(|v| all_literals(&v)),
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().all(|e| {
            // `node.values + node.keys` covers pairs only; a `**splat` adds nothing.
            e.as_assoc_node().is_none_or(|p| all_literals(&p.key()) && all_literals(&p.value()))
        }),
        _ if is_parser_dstr(node) => false,
        Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. } => false,
        _ => helpers::literal_node::literal(node),
    }
}

/// Collects `(operator start, rhs)` of every `equals_asgn?` node under a condition, not entering
/// block or def bodies.
struct Finder<'pr> {
    found: Vec<(usize, Node<'pr>)>,
}

macro_rules! asgn {
    ($fn:ident, $ty:ident, $walk:ident) => {
        fn $fn(&mut self, node: &ruby_prism::$ty<'pr>) {
            self.found.push((node.operator_loc().start_offset(), node.value()));
            ruby_prism::$walk(self, node);
        }
    };
}

impl<'pr> Visit<'pr> for Finder<'pr> {
    asgn!(visit_local_variable_write_node, LocalVariableWriteNode, visit_local_variable_write_node);
    asgn!(visit_instance_variable_write_node, InstanceVariableWriteNode, visit_instance_variable_write_node);
    asgn!(visit_class_variable_write_node, ClassVariableWriteNode, visit_class_variable_write_node);
    asgn!(visit_global_variable_write_node, GlobalVariableWriteNode, visit_global_variable_write_node);
    asgn!(visit_constant_write_node, ConstantWriteNode, visit_constant_write_node);
    asgn!(visit_constant_path_write_node, ConstantPathWriteNode, visit_constant_path_write_node);
    asgn!(visit_multi_write_node, MultiWriteNode, visit_multi_write_node);

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        if let Some(p) = node.parameters() {
            self.visit(&p);
        }
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        if let Some(p) = node.parameters() {
            self.visit(&p);
        }
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        if let Some(r) = node.receiver() {
            self.visit(&r);
        }
        if let Some(p) = node.parameters() {
            self.visit_parameters_node(&p);
        }
    }
}

impl Cop for LiteralAssignmentInCondition {
    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let condition = match node {
            Node::IfNode { .. } => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                node.as_if_node().unwrap().predicate()
            }
            Node::UnlessNode { .. } => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                node.as_unless_node().unwrap().predicate()
            }
            Node::WhileNode { .. } => {
                let w = node.as_while_node().unwrap();
                if w.is_begin_modifier() {
                    return;
                }
                w.predicate()
            }
            _ => {
                let u = node.as_until_node().unwrap();
                if u.is_begin_modifier() {
                    return;
                }
                u.predicate()
            }
        };
        let mut finder = Finder { found: Vec::new() };
        finder.visit(&condition);
        for (op_start, rhs) in finder.found {
            if !all_literals(&rhs) {
                continue;
            }
            // parallel_assignment_with_splat_operator?
            if rhs.as_array_node().is_some_and(|a| a.elements().iter().next().is_some_and(|v| v.as_splat_node().is_some())) {
                continue;
            }
            let msg = format!(
                "Don't use literal assignment `= {}` in conditional, should be `==` or non-literal operand.",
                node_text(&rhs)
            );
            cx.add(op_start, rhs.location().end_offset(), msg);
        }
    }
}
