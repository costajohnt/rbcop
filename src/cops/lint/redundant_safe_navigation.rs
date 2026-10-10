use crate::prelude::*;

pub const NAME: &str = "Lint/RedundantSafeNavigation";
pub static COP: RedundantSafeNavigation = RedundantSafeNavigation;
pub struct RedundantSafeNavigation;

const MSG: &str = "Redundant safe navigation detected, use `.` instead.";
const MSG_LITERAL: &str = "Redundant safe navigation with default literal detected.";
const MSG_NON_NIL: &str =
    "Redundant safe navigation on non-nil receiver (detected by analyzing previous code/method invocations).";


impl Cop for RedundantSafeNavigation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::OR_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node.as_call_node() {
            Some(call) => on_csend(cx, &call),
            None => on_or(cx, node),
        }
    }
}

fn on_csend(cx: &mut Cx, call: &ruby_prism::CallNode) {
    if !is_csend(call) {
        return;
    }
    let dot = call.call_operator_loc().unwrap();
    let (s, e) = (dot.start_offset(), dot.end_offset());
    let Some(receiver) = call.receiver() else { return };
    if cx.cfg.bool("InferNonNilReceiver").unwrap_or(false) {
        let additional = cx.cfg.strings("AdditionalNilMethods");
        if helpers::nil_receiver_checker::cant_be_nil(cx.src, cx.ancestors, call, &additional) {
            cx.add_fix(s, e, MSG_NON_NIL, |c| c.replace(s, e, "."));
            return;
        }
    }
    if guarded_by_nil_receiver(cx, call, &receiver) {
        return;
    }
    cx.add_fix(s, e, MSG, |c| c.replace(s, e, "."));
}

fn guarded_by_nil_receiver(cx: &Cx, call: &ruby_prism::CallNode, receiver: &Node) -> bool {
    if assume_receiver_instance_exists(receiver) {
        return false;
    }
    let guaranteed_instance = guaranteed_instance(receiver);
    if !guaranteed_instance && !check(cx, call) {
        return true;
    }
    respond_to_nil_method(call) && !guaranteed_instance
}

fn assume_receiver_instance_exists(receiver: &Node) -> bool {
    let short_name = match receiver {
        Node::ConstantReadNode { .. } => Some(receiver.as_constant_read_node().unwrap().name().as_slice()),
        Node::ConstantPathNode { .. } => receiver.as_constant_path_node().unwrap().name().map(|n| n.as_slice()),
        _ => None,
    };
    if let Some(name) = short_name {
        // SNAKE_CASE = /\A[[:digit:][:upper:]_]+\z/
        let snake = !name.is_empty() && std::str::from_utf8(name).is_ok_and(|n| n.chars().all(|c| c.is_numeric() || c.is_uppercase() || c == '_'));
        if !snake {
            return true;
        }
    }
    receiver.as_self_node().is_some()
        || (helpers::literal_node::literal(receiver) && receiver.as_nil_node().is_none())
}

fn guaranteed_instance(receiver: &Node) -> bool {
    let Some(call) = receiver.as_call_node() else { return false };
    !is_csend(&call) && matches!(method_name(&call), "to_s" | "to_i" | "to_f" | "to_a" | "to_h")
}

/// `(csend _ :respond_to? (sym %NIL_METHODS))`
fn respond_to_nil_method(call: &ruby_prism::CallNode) -> bool {
    if method_name(call) != "respond_to?" || call.block().is_some() {
        return false;
    }
    match call_args(call).as_slice() {
        [Node::SymbolNode { .. }] => {
            let args = call_args(call);
            let sym = args[0].as_symbol_node().unwrap();
            std::str::from_utf8(sym.unescaped()).is_ok_and(helpers::nil_methods::nil_object_method)
        }
        _ => false,
    }
}

fn check(cx: &Cx, call: &ruby_prism::CallNode) -> bool {
    if !helpers::allowed::allowed_method(cx.cfg, method_name(call)) {
        return false;
    }
    let Some(parent) = cx.ancestors.last() else { return false };
    let me = call.location();
    let me = (me.start_offset(), me.end_offset());
    // condition?(parent, node): parent.condition == node (structural).
    let condition = match parent {
        Node::IfNode { .. } => Some(parent.as_if_node().unwrap().predicate()),
        Node::UnlessNode { .. } => Some(parent.as_unless_node().unwrap().predicate()),
        Node::WhileNode { .. } => Some(parent.as_while_node().unwrap().predicate()),
        Node::UntilNode { .. } => Some(parent.as_until_node().unwrap().predicate()),
        Node::CaseNode { .. } => parent.as_case_node().unwrap().predicate(),
        Node::CaseMatchNode { .. } => parent.as_case_match_node().unwrap().predicate(),
        _ => None,
    };
    if let Some(c) = condition {
        let l = c.location();
        if helpers::node_equality::same(cx.src, (l.start_offset(), l.end_offset()), me) {
            return true;
        }
    }
    match parent {
        Node::AndNode { .. } | Node::OrNode { .. } => true,
        Node::CallNode { .. } => {
            let p = parent.as_call_node().unwrap();
            !is_csend(&p) && p.receiver().is_some() && method_name(&p) == "!"
        }
        _ => false,
    }
}

fn on_or(cx: &mut Cx, node: &Node) {
    let or = node.as_or_node().unwrap();
    let (lhs, rhs) = (or.left(), or.right());
    // (or (any_block $(csend _ :to_h) ...) (hash)) or (or $(csend _ :m) default)
    let Some(call) = lhs.as_call_node() else { return };
    if !is_csend(&call) || !send_args(&call).is_empty() {
        return;
    }
    let with_block = call.block().is_some_and(|b| b.as_block_node().is_some());
    let name = method_name(&call);
    let matches = match name {
        "to_h" => rhs.as_hash_node().is_some_and(|h| h.elements().iter().next().is_none()),
        _ if with_block => false,
        "to_a" => rhs.as_array_node().is_some_and(|a| a.elements().iter().next().is_none()),
        "to_i" => rhs.as_integer_node().is_some() && int_zero(node_text(&rhs)),
        "to_f" => rhs.as_float_node().is_some() && node_text(&rhs).replace('_', "").parse::<f64>() == Ok(0.0),
        "to_s" => rhs.as_string_node().is_some_and(|s| s.unescaped().is_empty()),
        _ => false,
    };
    if !matches {
        return;
    }
    let dot = call.call_operator_loc().unwrap();
    let (ds, de) = (dot.start_offset(), dot.end_offset());
    let end = node.location().end_offset();
    let lhs_end = lhs.location().end_offset();
    cx.add_fix(ds, end, MSG_LITERAL, |c| {
        c.replace(ds, de, ".");
        c.remove(lhs_end, end);
    });
}

fn int_zero(text: &str) -> bool {
    let t = text.replace('_', "");
    let t = t.trim_start_matches(['-', '+']);
    let digits = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .or_else(|| t.strip_prefix("0b"))
        .or_else(|| t.strip_prefix("0B"))
        .or_else(|| t.strip_prefix("0o"))
        .or_else(|| t.strip_prefix("0O"))
        .or_else(|| t.strip_prefix("0d"))
        .or_else(|| t.strip_prefix("0D"))
        .unwrap_or(t);
    !digits.is_empty() && digits.chars().all(|c| c == '0')
}
