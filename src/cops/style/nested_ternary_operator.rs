use crate::helpers::parser_parent::is_ternary;
use crate::helpers::range_help::{Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use ruby_prism::Visit;
use std::cell::Cell;

pub const NAME: &str = "Style/NestedTernaryOperator";
pub static COP: NestedTernaryOperator = NestedTernaryOperator;
pub struct NestedTernaryOperator;

const MSG: &str = "Ternary operators must not be nested. Prefer `if` or `else` constructs instead.";

/// `ignore_node`d outer ternaries.
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

impl Cop for NestedTernaryOperator {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(if_node) = node.as_if_node().filter(|_| is_ternary(node)) else { return };
        let mut nested = Ternaries(Vec::new());
        ruby_prism::visit_if_node(&mut nested, &if_node);
        if nested.0.is_empty() {
            return;
        }
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let ws = Surrounding { whitespace: true, ..Default::default() };
        let q = if_node.then_keyword_loc().unwrap();
        let q = range_with_surrounding_space(cx.src.text, q.start_offset(), q.end_offset(), ws);
        let colon = if_node.subsequent().and_then(|c| c.as_else_node()).map(|c| c.else_keyword_loc());
        let colon = colon.map(|c| range_with_surrounding_space(cx.src.text, c.start_offset(), c.end_offset(), ws));
        // remove_parentheses: a parenthesized `begin` if branch loses its own `(` and `)`.
        let parens = if_node
            .statements()
            .and_then(|s| s.body().iter().next())
            .and_then(|b| b.as_parentheses_node())
            .map(|p| (p.opening_loc(), p.closing_loc()));
        let heredoc = helpers::heredoc::last_heredoc_end(node).is_some();
        let modifier = modifier_position(node, cx.ancestors);
        for (ns, ne) in nested.0 {
            let ignored = cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= s && e <= b);
            let ran = Cell::new(false);
            cx.add_fix(ns, ne, MSG, |c| {
                if ignored || heredoc {
                    return;
                }
                c.replace(q.0, q.1, "\n");
                if let Some(colon) = colon {
                    c.replace(colon.0, colon.1, "\nelse\n");
                }
                if let Some((open, close)) = &parens {
                    c.remove(open.start_offset(), open.end_offset());
                    c.remove(close.start_offset(), close.end_offset());
                }
                if modifier {
                    c.wrap(s, e, "(if ", "\nend)");
                } else {
                    c.wrap(s, e, "if ", "\nend");
                }
                ran.set(true);
            });
            if ran.get() {
                cx.state::<Ignored>().0.push((s, e));
            }
        }
    }
}

/// `modifier_position?`: an `if` right after `return`, as the operand of `not` or `defined?`, or
/// as the first argument of a call without parentheses would be parsed as a modifier.
fn modifier_position(node: &Node, ancestors: &[Node]) -> bool {
    let Some(parent) = helpers::parser_parent::parser_parent(ancestors) else { return false };
    let start = node.location().start_offset();
    let first_is_node = |args: Option<ruby_prism::ArgumentsNode>| {
        args.and_then(|a| a.arguments().iter().next()).is_some_and(|f| f.location().start_offset() == start)
    };
    match parent {
        Node::ReturnNode { .. } | Node::BreakNode { .. } | Node::NextNode { .. } => true,
        Node::CallNode { .. } => {
            let c = parent.as_call_node().unwrap();
            if c.closing_loc().is_some_and(|l| l.as_slice() == b")") {
                return false;
            }
            let prefix_not = method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"not");
            prefix_not || first_is_node(c.arguments())
        }
        Node::SuperNode { .. } => {
            let s = parent.as_super_node().unwrap();
            s.rparen_loc().is_none() && first_is_node(s.arguments())
        }
        Node::YieldNode { .. } => {
            let y = parent.as_yield_node().unwrap();
            y.rparen_loc().is_none() && first_is_node(y.arguments())
        }
        Node::DefinedNode { .. } => {
            let d = parent.as_defined_node().unwrap();
            d.rparen_loc().is_none() && d.value().location().start_offset() == start
        }
        _ => false,
    }
}

/// Ternary descendants, pre-order.
struct Ternaries(Vec<(usize, usize)>);

impl<'pr> Visit<'pr> for Ternaries {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        if node.if_keyword_loc().is_none() {
            let loc = node.location();
            self.0.push((loc.start_offset(), loc.end_offset()));
        }
        ruby_prism::visit_if_node(self, node);
    }
}
