//! rubocop-ast `Node#pure?` on Prism nodes: conservative, false when unsure.

use crate::prelude::*;

pub fn pure(node: &Node) -> bool {

    let stmts = |s: Option<ruby_prism::StatementsNode>| s.is_none_or(|s| s.body().iter().all(|n| pure(&n)));
    let opt = |n: Option<Node>| n.is_none_or(|n| pure(&n));
    match node {
        // :__FILE__ :__LINE__ :const :cvar :defined? :false :float :gvar :int :ivar :lvar :nil
        // :str :sym :true (a multi-line str or sym is a dstr/dsym of pure parts too).
        Node::SourceFileNode { .. }
        | Node::SourceLineNode { .. }
        | Node::ConstantReadNode { .. }
        | Node::ConstantPathNode { .. }
        | Node::ClassVariableReadNode { .. }
        | Node::DefinedNode { .. }
        | Node::FalseNode { .. }
        | Node::FloatNode { .. }
        | Node::GlobalVariableReadNode { .. }
        | Node::IntegerNode { .. }
        | Node::InstanceVariableReadNode { .. }
        | Node::LocalVariableReadNode { .. }
        | Node::NilNode { .. }
        | Node::StringNode { .. }
        | Node::SymbolNode { .. }
        | Node::TrueNode { .. }
        | Node::RegularExpressionNode { .. } => true,
        Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            pure(&n.left()) && pure(&n.right())
        }
        Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            pure(&n.left()) && pure(&n.right())
        }
        Node::ArrayNode { .. } => all_pure(node.as_array_node().unwrap().elements().iter()),
        Node::HashNode { .. } => all_pure(node.as_hash_node().unwrap().elements().iter()),
        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            pure(&n.key()) && pure(&n.value())
        }
        Node::ParenthesesNode { .. } => opt(node.as_parentheses_node().unwrap().body()),
        Node::StatementsNode { .. } => stmts(node.as_statements_node()),
        Node::InterpolatedStringNode { .. } => all_pure(node.as_interpolated_string_node().unwrap().parts().iter()),
        Node::InterpolatedSymbolNode { .. } => all_pure(node.as_interpolated_symbol_node().unwrap().parts().iter()),
        Node::InterpolatedRegularExpressionNode { .. } => {
            all_pure(node.as_interpolated_regular_expression_node().unwrap().parts().iter())
        }
        Node::EmbeddedStatementsNode { .. } => stmts(node.as_embedded_statements_node().unwrap().statements()),
        Node::EmbeddedVariableNode { .. } => pure(&node.as_embedded_variable_node().unwrap().variable()),
        Node::RangeNode { .. } => {
            let n = node.as_range_node().unwrap();
            opt(n.left()) && opt(n.right())
        }
        Node::FlipFlopNode { .. } => {
            let n = node.as_flip_flop_node().unwrap();
            opt(n.left()) && opt(n.right())
        }
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            pure(&n.predicate()) && stmts(n.statements()) && opt(n.subsequent())
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            pure(&n.predicate()) && stmts(n.statements()) && n.else_clause().is_none_or(|e| stmts(e.statements()))
        }
        Node::ElseNode { .. } => stmts(node.as_else_node().unwrap().statements()),
        Node::CaseNode { .. } => {
            let n = node.as_case_node().unwrap();
            opt(n.predicate())
                && all_pure(n.conditions().iter())
                && n.else_clause().is_none_or(|e| stmts(e.statements()))
        }
        Node::WhenNode { .. } => {
            let n = node.as_when_node().unwrap();
            all_pure(n.conditions().iter()) && stmts(n.statements())
        }
        Node::WhileNode { .. } => {
            let n = node.as_while_node().unwrap();
            pure(&n.predicate()) && stmts(n.statements())
        }
        Node::UntilNode { .. } => {
            let n = node.as_until_node().unwrap();
            pure(&n.predicate()) && stmts(n.statements())
        }
        Node::BeginNode { .. } => {
            let n = node.as_begin_node().unwrap();
            n.rescue_clause().is_none()
                && stmts(n.statements())
                && n.else_clause().is_none_or(|e| stmts(e.statements()))
                && n.ensure_clause().is_none_or(|e| stmts(e.statements()))
        }
        _ => false,
    }
}

fn all_pure<'pr>(mut nodes: impl Iterator<Item = Node<'pr>>) -> bool {
    nodes.all(|n| pure(&n))
}
