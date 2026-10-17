use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/InstanceVariable";
pub static COP: InstanceVariable = InstanceVariable;
pub struct InstanceVariable;

/// dynamic_class? `(block (send (const nil? :Class) :new ...) ...)` or custom_matcher?
/// `(block {(send nil? :matcher sym) (send (const (const nil? :RSpec) :Matchers) :define sym)} ...)`
fn allowing_block(target: f64, call: &CallNode) -> bool {
    let sym = |n: &Node| n.as_symbol_node().is_some() && !is_parser_dsym(n, target);
    if is_csend(call) {
        return false;
    }
    let args = send_args(call);
    let name = method_name(call);
    match call.receiver() {
        Some(r) if name == "new" => r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Class"),
        Some(r) if name == "define" => {
            matches!(&args[..], [a] if sym(a))
                && r.as_constant_path_node().is_some_and(|p| {
                    p.name().is_some_and(|n| n.as_slice() == b"Matchers")
                        && p.parent().and_then(|q| q.as_constant_read_node()).is_some_and(|q| q.name().as_slice() == b"RSpec")
                })
        }
        None if name == "matcher" => matches!(&args[..], [a] if sym(a)),
        _ => false,
    }
}

impl Cop for InstanceVariable {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let assignment_only = cx.cfg.bool("AssignmentOnly") == Some(true);
        for group in helpers::rspec::top_level_groups(&l, node) {
            struct V {
                target: f64,
                /// nesting of plain blocks that allow instance variables
                allowed: usize,
                reads: Vec<((usize, usize), Vec<u8>)>,
                assigned: Vec<Vec<u8>>,
            }
            impl<'pr> Visit<'pr> for V {
                fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                    let allow = helpers::rspec::block_of(n).is_some_and(|b| helpers::rspec::is_plain(&b)) && allowing_block(self.target, n);
                    self.allowed += usize::from(allow);
                    ruby_prism::visit_call_node(self, n);
                    self.allowed -= usize::from(allow);
                }
                fn visit_instance_variable_read_node(&mut self, n: &ruby_prism::InstanceVariableReadNode<'pr>) {
                    if self.allowed == 0 {
                        self.reads.push((helpers::rspec::node_range(&n.as_node()), n.name().as_slice().to_vec()));
                    }
                }
                fn visit_instance_variable_write_node(&mut self, n: &ruby_prism::InstanceVariableWriteNode<'pr>) {
                    self.assigned.push(n.name().as_slice().to_vec());
                    ruby_prism::visit_instance_variable_write_node(self, n);
                }
                fn visit_instance_variable_operator_write_node(&mut self, n: &ruby_prism::InstanceVariableOperatorWriteNode<'pr>) {
                    self.assigned.push(n.name().as_slice().to_vec());
                    ruby_prism::visit_instance_variable_operator_write_node(self, n);
                }
                fn visit_instance_variable_or_write_node(&mut self, n: &ruby_prism::InstanceVariableOrWriteNode<'pr>) {
                    self.assigned.push(n.name().as_slice().to_vec());
                    ruby_prism::visit_instance_variable_or_write_node(self, n);
                }
                fn visit_instance_variable_and_write_node(&mut self, n: &ruby_prism::InstanceVariableAndWriteNode<'pr>) {
                    self.assigned.push(n.name().as_slice().to_vec());
                    ruby_prism::visit_instance_variable_and_write_node(self, n);
                }
                fn visit_instance_variable_target_node(&mut self, n: &ruby_prism::InstanceVariableTargetNode<'pr>) {
                    self.assigned.push(n.name().as_slice().to_vec());
                }
            }
            let mut v = V { target: cx.src.target_ruby, allowed: 0, reads: Vec::new(), assigned: Vec::new() };
            v.visit(&group);
            let (reads, assigned) = (v.reads, v.assigned);
            for ((s, e), name) in reads {
                if assignment_only && !assigned.contains(&name) {
                    continue;
                }
                cx.add(
                    s,
                    e,
                    "Avoid instance variables - use let, a method call, or a local variable (if possible).",
                );
            }
        }
    }
}
