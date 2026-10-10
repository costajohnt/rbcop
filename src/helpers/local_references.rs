//! `node.each_descendant(:lvar, :lvasgn)` names, skipping an `lvar` whose parent is a
//! splat / kwsplat / block_pass when asked (Style/ArgumentsForwarding's
//! `non_splat_or_block_pass_lvar_references`).

use crate::prelude::*;
use ruby_prism::Visit;
use std::collections::HashSet;

struct Refs<'pr> {
    skip_forwarded: bool,
    /// The visited root itself (each_descendant excludes it).
    root: (usize, usize),
    in_pattern: bool,
    names: HashSet<&'pr [u8]>,
}

impl<'pr> Refs<'pr> {
    fn add(&mut self, node: &Node, name: &'pr [u8]) {
        let l = node.location();
        if (l.start_offset(), l.end_offset()) != self.root {
            self.names.insert(name);
        }
    }

    fn forwarded(&self, e: Option<Node>) -> bool {
        self.skip_forwarded && e.is_some_and(|e| e.as_local_variable_read_node().is_some())
    }
}

impl<'pr> Visit<'pr> for Refs<'pr> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.add(&node.as_node(), node.name().as_slice());
    }
    fn visit_splat_node(&mut self, node: &ruby_prism::SplatNode<'pr>) {
        if !self.forwarded(node.expression()) {
            ruby_prism::visit_splat_node(self, node);
        }
    }
    fn visit_assoc_splat_node(&mut self, node: &ruby_prism::AssocSplatNode<'pr>) {
        if !self.forwarded(node.value()) {
            ruby_prism::visit_assoc_splat_node(self, node);
        }
    }
    fn visit_block_argument_node(&mut self, node: &ruby_prism::BlockArgumentNode<'pr>) {
        if !self.forwarded(node.expression()) {
            ruby_prism::visit_block_argument_node(self, node);
        }
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.add(&node.as_node(), node.name().as_slice());
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.add(&node.as_node(), node.name().as_slice());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.add(&node.as_node(), node.name().as_slice());
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.add(&node.as_node(), node.name().as_slice());
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        // Inside a pattern this is parser `match_var`, not `lvasgn`.
        if !self.in_pattern {
            self.add(&node.as_node(), node.name().as_slice());
        }
    }
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        // parser `match_with_lvasgn` has no lvasgn children
        self.visit_call_node(&node.call());
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        let saved = std::mem::replace(&mut self.in_pattern, true);
        self.visit(&node.pattern());
        self.in_pattern = saved;
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
        let saved = std::mem::replace(&mut self.in_pattern, true);
        self.visit(&node.pattern());
        self.in_pattern = saved;
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
        let saved = std::mem::replace(&mut self.in_pattern, true);
        self.visit(&node.pattern());
        self.in_pattern = saved;
    }
}

/// Names of `body.each_descendant(:lvar, :lvasgn)`; with `skip_forwarded`, an `lvar` directly
/// under a splat, kwsplat or block_pass is left out. `body` is a def body as Prism gives it: a
/// one-statement StatementsNode stands for that statement, which is excluded like the root.
pub fn lvar_lvasgn_names<'pr>(body: &Node<'pr>, skip_forwarded: bool) -> HashSet<&'pr [u8]> {
    let root = match body.as_statements_node().map(|s| s.body()) {
        Some(b) if b.len() == 1 => b.iter().next().unwrap().location(),
        _ => body.location(),
    };
    let mut v = Refs {
        skip_forwarded,
        root: (root.start_offset(), root.end_offset()),
        in_pattern: false,
        names: HashSet::new(),
    };
    v.visit(body);
    v.names
}
