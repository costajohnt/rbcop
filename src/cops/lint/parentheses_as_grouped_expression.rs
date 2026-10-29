use crate::prelude::*;

pub const NAME: &str = "Lint/ParenthesesAsGroupedExpression";
pub static COP: ParenthesesAsGroupedExpression = ParenthesesAsGroupedExpression;
pub struct ParenthesesAsGroupedExpression;

/// invalid_bare_argument? on a parenthesized first argument: unwrap single-expression parens,
/// then `and`/`or`, `x rescue y`, `not x`, or a modifier `if`/`unless`/`while`/`until`.
fn invalid_bare_argument(paren: &ruby_prism::ParenthesesNode) -> bool {
    let mut node = match paren.body() {
        None => return false,
        Some(b) => match b.as_statements_node() {
            Some(s) if s.body().iter().count() == 1 => s.body().iter().next().unwrap(),
            Some(_) => return false,
            None => b,
        },
    };
    while let Some(p) = node.as_parentheses_node() {
        node = match p.body() {
            None => return false,
            Some(b) => match b.as_statements_node() {
                Some(s) if s.body().iter().count() == 1 => s.body().iter().next().unwrap(),
                Some(_) => return false,
                None => b,
            },
        };
    }
    keyword_operator_or_modifier(&node) || command_call_operand(&node)
}

/// keyword_operator? || modifier_expression?
fn keyword_operator_or_modifier(node: &Node) -> bool {
    if let Some(a) = node.as_and_node() {
        return a.operator_loc().as_slice() == b"and";
    }
    if let Some(o) = node.as_or_node() {
        return o.operator_loc().as_slice() == b"or";
    }
    if node.as_rescue_modifier_node().is_some() {
        return true;
    }
    if let Some(c) = node.as_call_node() {
        return !is_csend(&c) && method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"not");
    }
    if let Some(i) = node.as_if_node() {
        return i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && i.end_keyword_loc().is_none();
    }
    if let Some(u) = node.as_unless_node() {
        return u.end_keyword_loc().is_none();
    }
    if let Some(w) = node.as_while_node() {
        return w.closing_loc().is_none() && !w.is_begin_modifier();
    }
    if let Some(u) = node.as_until_node() {
        return u.closing_loc().is_none() && !u.is_begin_modifier();
    }
    false
}

/// command_call_operand?: an assignment (or setter/index assignment) whose value, or a `!` whose
/// operand, is a command call (possibly through more of them).
fn command_call_operand(node: &Node) -> bool {
    if let Some((_, value)) = assignment_parts(node) {
        return command_call_value(&value);
    }
    let Some(call) = node.as_call_node() else { return false };
    let name = method_name(&call);
    // call_type? && assignment_method?: `children.last` is the last argument.
    if name.ends_with('=') && !matches!(name, "==" | "===" | "!=" | "<=" | ">=") {
        return send_args(&call).last().is_some_and(command_call_value);
    }
    // send_type? && prefix_bang?
    if !is_csend(&call) && name == "!" && call.message_loc().is_some_and(|m| m.as_slice() == b"!") {
        return call.receiver().is_some_and(|r| command_call_value(&r));
    }
    false
}

fn command_call_value(node: &Node) -> bool {
    command_call(node) || command_call_operand(node)
}

/// command_call?: a send/csend, `super` or `yield` (a block's send node for a block) with arguments
/// and without parentheses, not an operator method.
fn command_call(node: &Node) -> bool {
    if let Some(call) = node.as_call_node() {
        return !send_args(&call).is_empty()
            && call.closing_loc().is_none_or(|c| c.as_slice() != b")")
            && !helpers::node_types::is_operator_method(method_name(&call));
    }
    if let Some(sup) = node.as_super_node() {
        let block_pass = sup.block().is_some_and(|b| b.as_block_argument_node().is_some());
        return (sup.arguments().is_some() || block_pass) && sup.rparen_loc().is_none();
    }
    if let Some(y) = node.as_yield_node() {
        return y.arguments().is_some() && y.rparen_loc().is_none();
    }
    false
}

impl Cop for ParenthesesAsGroupedExpression {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let args = send_args(&call);
        // Only a `(...)` first argument both is a `parenthesized_call?` and starts with `(`
        // without making `chained_calls?` true.
        let [arg] = args.as_slice() else { return };
        let Some(paren) = arg.as_parentheses_node() else { return };
        if helpers::node_types::is_operator_method(method_name(&call))
            || call.equal_loc().is_some()
            || call.opening_loc().is_some()
            || invalid_bare_argument(&paren)
        {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let arg_start = arg.location().start_offset();
        if arg_start <= sel.end_offset() {
            return;
        }
        let sel_end = sel.end_offset();
        let msg = format!("`{}` interpreted as grouped expression.", node_text(arg));
        cx.add_fix(sel_end, arg_start, msg, |c| c.remove(sel_end, arg_start));
    }
}
