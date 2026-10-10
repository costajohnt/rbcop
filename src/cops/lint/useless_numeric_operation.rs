use crate::prelude::*;

pub const NAME: &str = "Lint/UselessNumericOperation";
pub static COP: UselessNumericOperation = UselessNumericOperation;
pub struct UselessNumericOperation;

const MSG: &str = "Do not apply inconsequential numeric operations to variables.";

/// `useless?(operation, number)`.
fn useless(op: &[u8], number: i128) -> bool {
    match number {
        0 => matches!(op, b"+" | b"-"),
        1 => matches!(op, b"*" | b"/" | b"**"),
        _ => false,
    }
}

/// `(int $_)`: an integer literal's value (Prism folds `-1`, as parser does).
fn int_value(node: &Node) -> Option<i128> {
    node.as_integer_node()?;
    int_literal_value(node)
}

/// `{lvar ivar cvar gvar const (send nil? _)}`.
fn variable_like(node: &Node) -> bool {
    match kind::of(node) {
        kind::LOCAL_VARIABLE_READ_NODE
        | kind::INSTANCE_VARIABLE_READ_NODE
        | kind::CLASS_VARIABLE_READ_NODE
        | kind::GLOBAL_VARIABLE_READ_NODE
        | kind::CONSTANT_READ_NODE
        | kind::CONSTANT_PATH_NODE => true,
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            c.receiver().is_none() && c.arguments().is_none() && c.block().is_none()
        }
        _ => false,
    }
}

impl Cop for UselessNumericOperation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CONSTANT_OPERATOR_WRITE_NODE,
            kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        if let Some(call) = node.as_call_node() {
            let Some(recv) = call.receiver() else { return };
            // A call with a literal block is a parser `block`.
            if call.block().is_some() || !variable_like(&recv) {
                return;
            }
            let args = call_args(&call);
            let [arg] = args.as_slice() else { return };
            let Some(n) = int_value(arg) else { return };
            if !useless(call.name().as_slice(), n) {
                return;
            }
            let replacement = node_text(&recv).to_string();
            cx.add_fix(start, end, MSG, |c| c.replace(start, end, &replacement));
            return;
        }
        let (var_end, op, value) = match kind::of(node) {
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE => {
                let w = node.as_local_variable_operator_write_node().unwrap();
                (w.name_loc().end_offset(), w.binary_operator(), w.value())
            }
            kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE => {
                let w = node.as_instance_variable_operator_write_node().unwrap();
                (w.name_loc().end_offset(), w.binary_operator(), w.value())
            }
            kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE => {
                let w = node.as_class_variable_operator_write_node().unwrap();
                (w.name_loc().end_offset(), w.binary_operator(), w.value())
            }
            kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE => {
                let w = node.as_global_variable_operator_write_node().unwrap();
                (w.name_loc().end_offset(), w.binary_operator(), w.value())
            }
            kind::CONSTANT_OPERATOR_WRITE_NODE => {
                let w = node.as_constant_operator_write_node().unwrap();
                (w.name_loc().end_offset(), w.binary_operator(), w.value())
            }
            _ => {
                let w = node.as_constant_path_operator_write_node().unwrap();
                (w.target().location().end_offset(), w.binary_operator(), w.value())
            }
        };
        let Some(n) = int_value(&value) else { return };
        if !useless(op.as_slice(), n) {
            return;
        }
        let var = &cx.src.text[start..var_end];
        let replacement = format!("{var} = {var}");
        cx.add_fix(start, end, MSG, |c| c.replace(start, end, &replacement));
    }
}
