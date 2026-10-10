use crate::prelude::*;

pub const NAME: &str = "Style/RedundantSelfAssignment";
pub static COP: RedundantSelfAssignment = RedundantSelfAssignment;
pub struct RedundantSelfAssignment;

const METHODS_RETURNING_SELF: &[&str] = &[
    "append", "clear", "collect!", "compare_by_identity", "concat", "delete_if", "fill", "initialize_copy", "insert",
    "keep_if", "map!", "merge!", "prepend", "push", "rehash", "replace", "reverse!", "rotate!", "shuffle!", "sort!",
    "sort_by!", "transform_keys!", "transform_values!", "unshift", "update",
];

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

impl Cop for RedundantSelfAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::CLASS_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_WRITE_NODE,
            kind::CALL_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(call) = node.as_call_node() {
            return on_send(cx, &call);
        }
        // on_lvasgn / on_ivasgn / on_cvasgn / on_gvasgn
        let (name, value, op) = if let Some(w) = node.as_local_variable_write_node() {
            (w.name(), w.value(), w.operator_loc())
        } else if let Some(w) = node.as_instance_variable_write_node() {
            (w.name(), w.value(), w.operator_loc())
        } else if let Some(w) = node.as_class_variable_write_node() {
            (w.name(), w.value(), w.operator_loc())
        } else {
            let w = node.as_global_variable_write_node().unwrap();
            (w.name(), w.value(), w.operator_loc())
        };
        let Some(rhs) = value.as_call_node() else { return };
        let method = method_name(&rhs);
        if !METHODS_RETURNING_SELF.contains(&method) {
            return;
        }
        let Some(receiver) = rhs.receiver() else { return };
        let receiver_name = match kind::of(node) {
            kind::LOCAL_VARIABLE_WRITE_NODE => receiver.as_local_variable_read_node().map(|r| r.name()),
            kind::INSTANCE_VARIABLE_WRITE_NODE => receiver.as_instance_variable_read_node().map(|r| r.name()),
            kind::CLASS_VARIABLE_WRITE_NODE => receiver.as_class_variable_read_node().map(|r| r.name()),
            _ => receiver.as_global_variable_read_node().map(|r| r.name()),
        };
        if receiver_name.is_none_or(|r| r.as_slice() != name.as_slice()) {
            return;
        }
        let msg = format!("Redundant self assignment detected. Method `{method}` modifies its receiver in place.");
        let (start, end) = range(node);
        let rhs_src = node_text(&value).to_string();
        cx.add_fix(op.start_offset(), op.end_offset(), msg, |c| c.replace(start, end, &rhs_src));
    }
}

/// on_send / on_csend: `foo.bar = foo.bar.concat(baz)`.
fn on_send(cx: &mut Cx, call: &ruby_prism::CallNode) {
    let name = method_name(call);
    // assignment_method?: ends with `=`, not a comparison.
    if !name.ends_with('=') || matches!(name, "==" | "!=" | "===" | "<=" | ">=") {
        return;
    }
    let Some(op) = call.equal_loc() else { return };
    if call.block().is_some() {
        return;
    }
    let args = call_args(call);
    let [arg] = args.as_slice() else { return };
    // (call (call %1 %2) #method_returning_self? ...)
    let Some(inner) = arg.as_call_node() else { return };
    if inner.block().is_some_and(|b| b.as_block_node().is_some()) || !METHODS_RETURNING_SELF.contains(&method_name(&inner)) {
        return;
    }
    let Some(getter) = inner.receiver().and_then(|r| r.as_call_node()) else { return };
    if method_name(&getter) != name.strip_suffix('=').unwrap() || getter.arguments().is_some() || getter.block().is_some() {
        return;
    }
    let same_receiver = match (call.receiver(), getter.receiver()) {
        (None, None) => true,
        (Some(a), Some(b)) => helpers::node_equality::same(cx.src, range(&a), range(&b)),
        _ => false,
    };
    if !same_receiver {
        return;
    }
    let msg = format!(
        "Redundant self assignment detected. Method `{}` modifies its receiver in place.",
        method_name(&inner)
    );
    let start = call.location().start_offset();
    let arg_start = arg.location().start_offset();
    cx.add_fix(op.start_offset(), op.end_offset(), msg, |c| c.remove(start, arg_start));
}
