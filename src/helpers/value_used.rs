//! rubocop-ast `Node#value_used?` over Prism ancestors.

use crate::helpers::parser_parent::parser_parent_index;
use crate::prelude::*;

/// `value_used?` of the node whose Prism ancestors are `ancestors` and which starts at `start`
/// (a call owning a block stands for the block, so pass the call's start).
pub fn value_used(ancestors: &[Node], start: usize) -> bool {
    let Some(pi) = parser_parent_index(ancestors, ancestors.len()) else { return false };
    let parent = &ancestors[pi];
    // A BlockNode's parser node starts at its owning call.
    let parent_start = if kind::of(parent) == kind::BLOCK_NODE && pi > 0 {
        ancestors[pi - 1].location().start_offset()
    } else {
        parent.location().start_offset()
    };
    let up = || value_used(&ancestors[..pi], parent_start);
    let last_statement = |stmts: Option<ruby_prism::StatementsNode>| {
        stmts.and_then(|s| s.body().iter().last()).is_some_and(|l| l.location().start_offset() == start)
    };
    match parent {
        Node::ArrayNode { .. }
        | Node::DefinedNode { .. }
        | Node::InterpolatedStringNode { .. }
        | Node::InterpolatedSymbolNode { .. }
        | Node::InterpolatedRegularExpressionNode { .. }
        | Node::InterpolatedXStringNode { .. }
        | Node::FlipFlopNode { .. }
        | Node::RangeNode { .. }
        | Node::HashNode { .. }
        | Node::KeywordHashNode { .. }
        | Node::AssocNode { .. }
        | Node::WhenNode { .. } => up(),
        Node::CallNode { .. } => {
            let c = parent.as_call_node().unwrap();
            // `not x` (parser `not`); `!x` is a send.
            if c.message_loc().is_some_and(|m| m.as_slice() == b"not") { up() } else { true }
        }
        Node::StatementsNode { .. } => {
            if last_statement(parent.as_statements_node()) {
                up()
            } else {
                false
            }
        }
        Node::ParenthesesNode { .. } => {
            let body = parent.as_parentheses_node().unwrap().body();
            let last = match body.as_ref().and_then(|b| b.as_statements_node()) {
                Some(s) => s.body().iter().last(),
                None => body,
            };
            if last.is_some_and(|l| l.location().start_offset() == start) { up() } else { false }
        }
        Node::EmbeddedStatementsNode { .. } => {
            if last_statement(parent.as_embedded_statements_node().unwrap().statements()) {
                up()
            } else {
                false
            }
        }
        Node::BeginNode { .. } => {
            let b = parent.as_begin_node().unwrap();
            if b.begin_keyword_loc().is_none()
                || b.rescue_clause().is_some()
                || b.ensure_clause().is_some()
                || b.else_clause().is_some()
            {
                return true;
            }
            if last_statement(b.statements()) { up() } else { false }
        }
        Node::ForNode { .. } => {
            let f = parent.as_for_node().unwrap();
            if f.statements().is_some_and(|s| s.location().start_offset() <= start) { up() } else { true }
        }
        Node::IfNode { .. } | Node::UnlessNode { .. } | Node::CaseNode { .. } => {
            let predicate = match parent {
                Node::IfNode { .. } => Some(parent.as_if_node().unwrap().predicate()),
                Node::UnlessNode { .. } => Some(parent.as_unless_node().unwrap().predicate()),
                _ => parent.as_case_node().unwrap().predicate(),
            };
            predicate.is_some_and(|p| p.location().start_offset() == start) || up()
        }
        Node::WhileNode { .. } => parent.as_while_node().unwrap().predicate().location().start_offset() == start,
        Node::UntilNode { .. } => parent.as_until_node().unwrap().predicate().location().start_offset() == start,
        _ => true,
    }
}
