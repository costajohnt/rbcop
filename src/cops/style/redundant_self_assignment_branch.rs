use crate::helpers::if_node::{Body, Else, parts};
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantSelfAssignmentBranch";
pub static COP: RedundantSelfAssignmentBranch = RedundantSelfAssignmentBranch;
pub struct RedundantSelfAssignmentBranch;

impl Cop for RedundantSelfAssignmentBranch {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::LOCAL_VARIABLE_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let asgn = node.as_local_variable_write_node().unwrap();
        let expression = asgn.value();
        // `if_type?`: if, unless, ternary (every form has an if and an else branch to look at).
        if !matches!(kind::of(&expression), kind::IF_NODE | kind::UNLESS_NODE) {
            return;
        }
        if !statement(node, cx.ancestors) {
            return;
        }
        let (condition, if_branch, else_part) = parts(&expression);
        let else_branch = match else_part {
            Some(Else::Elsif(_)) => return,
            Some(Else::Body(b)) => b,
            None => None,
        };
        if multiple_statements(&if_branch) || multiple_statements(&else_branch) {
            return;
        }
        let name = asgn.name().as_slice();
        let if_node = if_branch.as_ref().and_then(Body::single);
        let else_node = else_branch.as_ref().and_then(Body::single);
        let self_assign = |b: &Option<Node>| b.as_ref().is_some_and(|n| node_text(n).as_bytes() == name);
        let (offense, opposite, keyword) = if self_assign(&if_node) {
            (if_node.unwrap(), else_node, "unless")
        } else if self_assign(&else_node) {
            (else_node.unwrap(), if_node, "if")
        } else {
            return;
        };
        let src = cx.src.text;
        let mut replacement = format!(
            "{} {keyword} {}",
            opposite.as_ref().map_or("nil", node_text),
            node_text(&condition)
        );
        if let Some((opening, end, _)) = opposite.as_ref().and_then(helpers::heredoc::parts) {
            replacement.push_str(&src[opening.1..end.1]);
        }
        let (s, e) = (expression.location().start_offset(), expression.location().end_offset());
        let loc = offense.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), "Remove the self-assignment branch.", |c| {
            c.replace(s, e, &replacement)
        });
    }
}

/// `statement?`: the assignment is a statement, not a value something else uses.
fn statement(node: &Node, ancestors: &[Node]) -> bool {
    let Some(pi) = helpers::parser_parent::parser_parent_index(ancestors, ancestors.len()) else { return true };
    let parent = &ancestors[pi];
    // A multi-statement body is a parser `begin`: a statement unless it has `(` or `#{`.
    if parent.as_statements_node().is_some() {
        return pi == 0
            || !matches!(kind::of(&ancestors[pi - 1]), kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE);
    }
    let me = |n: Option<Node>| n.is_some_and(|n| n.location().start_offset() == node.location().start_offset());
    match parent {
        // STATEMENT_BODY_TYPES: kwbegin def defs block class module sclass rescue ensure (and a
        // multi-statement body, a parser `begin` without `loc.begin`).
        Node::BeginNode { .. }
        | Node::DefNode { .. }
        | Node::BlockNode { .. }
        | Node::LambdaNode { .. }
        | Node::ClassNode { .. }
        | Node::ModuleNode { .. }
        | Node::SingletonClassNode { .. }
        | Node::RescueNode { .. }
        | Node::EnsureNode { .. }
        | Node::ProgramNode { .. } => true,
        // `x rescue y = ...` is a modifier resbody; the rescued expression is the parser `rescue`.
        Node::RescueModifierNode { .. } => me(Some(parent.as_rescue_modifier_node().unwrap().expression())),
        Node::IfNode { .. } => {
            let i = parent.as_if_node().unwrap();
            i.if_keyword_loc().is_some() && !me(Some(i.predicate()))
        }
        Node::UnlessNode { .. } => !me(Some(parent.as_unless_node().unwrap().predicate())),
        Node::WhenNode { .. } => !parent.as_when_node().unwrap().conditions().iter().any(|c| me(Some(c))),
        Node::InNode { .. } => !me(Some(parent.as_in_node().unwrap().pattern())),
        Node::WhileNode { .. } => !me(Some(parent.as_while_node().unwrap().predicate())),
        Node::UntilNode { .. } => !me(Some(parent.as_until_node().unwrap().predicate())),
        Node::ForNode { .. } => {
            let f = parent.as_for_node().unwrap();
            !me(Some(f.index())) && !me(Some(f.collection()))
        }
        Node::CaseNode { .. } => me(parent.as_case_node().unwrap().else_clause().and_then(|e| e.statements()).map(|s| s.as_node())),
        Node::CaseMatchNode { .. } => {
            me(parent.as_case_match_node().unwrap().else_clause().and_then(|e| e.statements()).map(|s| s.as_node()))
        }
        _ => false,
    }
}

/// `branch.begin_type? && !branch.children.empty?`
fn multiple_statements(branch: &Option<Body>) -> bool {
    let Some(b) = branch else { return false };
    match b.single() {
        None => true,
        Some(n) => n.as_parentheses_node().is_some_and(|p| p.body().is_some()),
    }
}
