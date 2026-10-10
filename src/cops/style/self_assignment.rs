use crate::prelude::*;

pub const NAME: &str = "Style/SelfAssignment";
pub static COP: SelfAssignment = SelfAssignment;
pub struct SelfAssignment;

const OPS: &[&str] = &["+", "-", "*", "**", "/", "%", "^", "<<", ">>", "|", "&"];

/// `s(var_type, var_name)`: a read of the variable being assigned.
fn same_var(node: &Node, write_kind: usize, name: &[u8]) -> bool {
    match write_kind {
        kind::LOCAL_VARIABLE_WRITE_NODE => node.as_local_variable_read_node().is_some_and(|r| r.name().as_slice() == name),
        kind::INSTANCE_VARIABLE_WRITE_NODE => {
            node.as_instance_variable_read_node().is_some_and(|r| r.name().as_slice() == name)
        }
        _ => node.as_class_variable_read_node().is_some_and(|r| r.name().as_slice() == name),
    }
}

impl Cop for SelfAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::LOCAL_VARIABLE_WRITE_NODE, kind::INSTANCE_VARIABLE_WRITE_NODE, kind::CLASS_VARIABLE_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let k = kind::of(node);
        let (name, rhs, op_loc) = if let Some(w) = node.as_local_variable_write_node() {
            (w.name(), w.value(), w.operator_loc())
        } else if let Some(w) = node.as_instance_variable_write_node() {
            (w.name(), w.value(), w.operator_loc())
        } else {
            let w = node.as_class_variable_write_node().unwrap();
            (w.name(), w.value(), w.operator_loc())
        };
        let name = name.as_slice();
        // (operator, lhs, rhs) of the right-hand side
        let (op, new_rhs) = if let Some(call) = rhs.as_call_node() {
            if is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
                return;
            }
            let args = send_args(&call);
            let method = method_name(&call);
            if args.len() != 1 || !OPS.contains(&method) || !call.receiver().is_some_and(|r| same_var(&r, k, name)) {
                return;
            }
            (method.to_string(), node_text(&args[0]))
        } else if let Some(and) = rhs.as_and_node() {
            if !same_var(&and.left(), k, name) {
                return;
            }
            (text(&and.operator_loc()).to_string(), node_text(&and.right()))
        } else if let Some(or) = rhs.as_or_node() {
            if !same_var(&or.left(), k, name) {
                return;
            }
            (text(&or.operator_loc()).to_string(), node_text(&or.right()))
        } else {
            return;
        };
        let loc = node.location();
        let rl = rhs.location();
        let msg = format!("Use self-assignment shorthand `{op}=`.");
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
            c.insert_before(op_loc.start_offset(), op_loc.end_offset(), &op);
            c.replace(rl.start_offset(), rl.end_offset(), new_rhs);
        });
    }
}
