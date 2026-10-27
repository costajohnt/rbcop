//! RuboCop's EndlessMethodBodyHelp mixin (1.92): whether a method body can become the body of an
//! endless method definition without changing its meaning. Used by Style/SingleLineMethods; meant
//! for Style/EndlessMethod and Style/AccessModifierDeclarations too.

use crate::prelude::*;
use ruby_prism::Visit;

/// `argument_receiving_call(def_node)`: the call whose arguments include the def.
pub struct ArgumentCall {
    /// `call.arguments.one?`
    pub one_argument: bool,
    /// `call.parenthesized?`
    pub parenthesized: bool,
}

/// The call (send/csend) the def whose Prism ancestors are `ancestors` is an argument of.
pub fn argument_receiving_call(ancestors: &[Node]) -> Option<ArgumentCall> {
    let n = ancestors.len();
    ancestors.get(n.checked_sub(1)?)?.as_arguments_node()?;
    let call = ancestors.get(n.checked_sub(2)?)?.as_call_node()?;
    Some(ArgumentCall {
        one_argument: send_args(&call).len() == 1,
        parenthesized: call.closing_loc().is_some_and(|l| l.as_slice() == b")"),
    })
}

/// The parser arguments of a send / super / yield (block pass included).
fn arguments<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    if let Some(c) = node.as_call_node() {
        return Some(send_args(&c));
    }
    let (args, block) = match node {
        Node::SuperNode { .. } => {
            let s = node.as_super_node().unwrap();
            (s.arguments(), s.block())
        }
        Node::YieldNode { .. } => (node.as_yield_node().unwrap().arguments(), None),
        _ => return None,
    };
    let mut out: Vec<Node> = args.map(|a| a.arguments().iter().collect()).unwrap_or_default();
    out.extend(block.filter(|b| b.as_block_argument_node().is_some()));
    Some(out)
}

/// `command_call?(node)`: a call, super or yield with arguments and no parentheses (a call with a
/// block counts as its send node), not an operator method or setter.
pub fn command_call(node: &Node) -> bool {
    let Some(args) = arguments(node) else { return false };
    let close = match node {
        Node::CallNode { .. } => node.as_call_node().unwrap().closing_loc(),
        Node::SuperNode { .. } => node.as_super_node().unwrap().rparen_loc(),
        _ => node.as_yield_node().unwrap().rparen_loc(),
    };
    let (operator, setter) = node.as_call_node().map_or((false, false), |c| {
        (helpers::node_types::is_operator_method(method_name(&c)), c.equal_loc().is_some())
    });
    !args.is_empty() && close.is_none_or(|l| l.as_slice() != b")") && !operator && !setter
}

/// One step of `assigned_value`: the value of an assignment (setter sends included) or the body
/// of a rescue modifier.
fn assigned_step<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    if let Some(r) = node.as_rescue_modifier_node() {
        return Some(r.expression());
    }
    if let Some(c) = node.as_call_node().filter(|c| c.equal_loc().is_some()) {
        return send_args(&c).pop();
    }
    assignment_parts(node).map(|(_, value)| value)
}

/// `assigned_value(node)` for an assignment `node`.
fn assigned_value<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let mut cur = assigned_step(node)?;
    while let Some(next) = assigned_step(&cur) {
        cur = next;
    }
    Some(cur)
}

fn is_semantic(op: ruby_prism::Location) -> bool {
    matches!(op.as_slice(), b"and" | b"or")
}

/// `low_precedence_body?`: `def foo = bar and baz` parses as `(def foo = bar) and baz`.
fn low_precedence_body(body: &Node) -> bool {
    match body {
        Node::MatchPredicateNode { .. } | Node::MatchRequiredNode { .. } => true,
        Node::AndNode { .. } => is_semantic(body.as_and_node().unwrap().operator_loc()),
        Node::OrNode { .. } => is_semantic(body.as_or_node().unwrap().operator_loc()),
        // while_post / until_post, or a modifier `while` / `until`
        Node::WhileNode { .. } => body.as_while_node().unwrap().closing_loc().is_none(),
        Node::UntilNode { .. } => body.as_until_node().unwrap().closing_loc().is_none(),
        Node::IfNode { .. } => {
            let i = body.as_if_node().unwrap();
            i.if_keyword_loc().is_some() && i.end_keyword_loc().is_none()
        }
        Node::UnlessNode { .. } => body.as_unless_node().unwrap().end_keyword_loc().is_none(),
        _ => false,
    }
}

/// What the descendant checks need, gathered in one walk of the body (the body itself included).
#[derive(Default)]
struct Walk {
    body_end: usize,
    omitted_value_at_end: bool,
    anonymous_argument_at_end: bool,
    /// A `do...end` block whose send is a command call.
    do_end_command_block: bool,
    /// A `do...end` block (not a lambda literal).
    do_end_block: bool,
}

impl<'pr> Visit<'pr> for Walk {
    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode<'pr>) {
        if node.value().as_implicit_node().is_some() && node.location().end_offset() == self.body_end {
            self.omitted_value_at_end = true;
        }
        ruby_prism::visit_assoc_node(self, node);
    }
    fn visit_arguments_node(&mut self, node: &ruby_prism::ArgumentsNode<'pr>) {
        // forwarded_restarg `*` / forwarded_kwrestarg `**`
        for a in node.arguments().iter() {
            let anonymous = a.as_splat_node().is_some_and(|s| s.expression().is_none())
                || a.as_keyword_hash_node().is_some_and(|h| {
                    h.elements().iter().any(|e| e.as_assoc_splat_node().is_some_and(|s| s.value().is_none()))
                });
            if anonymous && a.location().end_offset() == self.body_end {
                self.anonymous_argument_at_end = true;
            }
        }
        ruby_prism::visit_arguments_node(self, node);
    }
    fn visit_block_argument_node(&mut self, node: &ruby_prism::BlockArgumentNode<'pr>) {
        if node.expression().is_none() && node.location().end_offset() == self.body_end {
            self.anonymous_argument_at_end = true;
        }
        ruby_prism::visit_block_argument_node(self, node);
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if let Some(b) = node.block().and_then(|b| b.as_block_node())
            && b.closing_loc().as_slice() == b"end"
        {
            self.do_end_block = true;
            if command_call(&node.as_node()) {
                self.do_end_command_block = true;
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'pr>) {
        if let Some(b) = node.block().and_then(|b| b.as_block_node())
            && b.closing_loc().as_slice() == b"end"
        {
            self.do_end_block = true;
            if command_call(&node.as_node()) {
                self.do_end_command_block = true;
            }
        }
        ruby_prism::visit_super_node(self, node);
    }
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode<'pr>) {
        if node.block().is_some_and(|b| b.closing_loc().as_slice() == b"end") {
            self.do_end_block = true;
        }
        ruby_prism::visit_forwarding_super_node(self, node);
    }
}

/// `endless_method_body?(body)` for the parser body of a def. `arg_call` is the def's
/// `argument_receiving_call`; `parenthesized_body` is set when the caller parenthesizes the body
/// (Style/SingleLineMethods' `command_body_in_argument?` override: `!require_parentheses?(body)`).
pub fn endless_method_body(body: &Node, arg_call: Option<&ArgumentCall>, target_ruby: f64, parenthesized_body: bool) -> bool {
    // `begin`, `kwbegin`, `rescue`, `ensure`, `masgn`
    if matches!(
        body,
        Node::StatementsNode { .. }
            | Node::ParenthesesNode { .. }
            | Node::BeginNode { .. }
            | Node::RescueModifierNode { .. }
            | Node::MultiWriteNode { .. }
    ) {
        return false;
    }
    if low_precedence_body(body) {
        return false;
    }
    let (_, body_end) = body_range(body);
    let mut w = Walk { body_end, ..Default::default() };
    w.visit(body);
    if w.omitted_value_at_end || w.anonymous_argument_at_end {
        return false;
    }
    let value = if is_assignment(body) { assigned_value(body) } else { None };
    // multiple_values?: `foo = 1, 2` and `foo = *bar` assign an array without brackets.
    if value.as_ref().and_then(|v| v.as_array_node()).is_some_and(|a| a.opening_loc().is_none()) {
        return false;
    }
    // command_disallowed_in_endless_body?
    if value.as_ref().is_some_and(command_call) {
        return false;
    }
    // command_body_in_argument? (`private def foo = bar baz` parses only on 4.0+, as the sole argument)
    if !parenthesized_body
        && command_call(body)
        && arg_call.is_some_and(|c| target_ruby < 4.0 || !c.one_argument)
    {
        return false;
    }
    // do_end_block_body_in_command_argument?
    if arg_call.is_some_and(|c| !c.parenthesized) && w.do_end_block {
        return false;
    }
    // negated_command_call?: `!foo bar`
    if let Some(c) = body.as_call_node()
        && method_name(&c) == "!"
        && c.message_loc().is_some_and(|m| m.as_slice() == b"!")
        && c.receiver().is_some_and(|r| command_call(&r))
    {
        return false;
    }
    !w.do_end_command_block
}
