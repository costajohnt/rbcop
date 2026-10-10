use crate::prelude::*;

pub const NAME: &str = "Lint/NumericOperationWithConstantResult";
pub static COP: NumericOperationWithConstantResult = NumericOperationWithConstantResult;
pub struct NumericOperationWithConstantResult;

const MSG: &str = "Numeric operation with a constant result detected.";

/// A captured operand: an `int` value or a method / variable name.
#[derive(PartialEq)]
enum Operand<'a> {
    Int(Option<i128>),
    Name(&'a [u8]),
}

/// `(call nil? $_)`: a receiverless call without arguments or block.
fn bare_call<'pr>(node: &Node<'pr>) -> Option<&'pr [u8]> {
    let c = node.as_call_node()?;
    (c.receiver().is_none() && c.arguments().is_none() && c.block().is_none()).then(|| c.name().as_slice())
}

fn constant_result(lhs: &Operand, op: &[u8], rhs: &Operand) -> Option<u8> {
    if *rhs == Operand::Int(Some(0)) {
        return match op {
            b"*" => Some(0),
            b"**" => Some(1),
            _ => None,
        };
    }
    (rhs == lhs && op == b"/").then_some(1)
}

impl Cop for NumericOperationWithConstantResult {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = node.location();
        let (s, e) = (l.start_offset(), l.end_offset());
        if let Some(w) = node.as_local_variable_operator_write_node() {
            let value = w.value();
            let rhs = if value.as_integer_node().is_some() {
                Operand::Int(int_literal_value(&value))
            } else if let Some(v) = value.as_local_variable_read_node() {
                Operand::Name(v.name().as_slice())
            } else {
                return;
            };
            let lhs_name = w.name().as_slice();
            let Some(result) = constant_result(&Operand::Name(lhs_name), w.binary_operator().as_slice(), &rhs) else {
                return;
            };
            let replacement = format!("{} = {result}", String::from_utf8_lossy(lhs_name));
            cx.add_fix(s, e, MSG, |c| c.replace(s, e, &replacement));
            return;
        }
        let call = node.as_call_node().unwrap();
        let op = call.name().as_slice();
        if !matches!(op, b"*" | b"/" | b"**") || is_csend(&call) {
            return;
        }
        let Some(lhs) = call.receiver().as_ref().and_then(bare_call) else { return };
        let args = call_args(&call);
        let [arg] = args.as_slice() else { return };
        let rhs = if arg.as_integer_node().is_some() {
            Operand::Int(int_literal_value(arg))
        } else if let Some(name) = bare_call(arg) {
            Operand::Name(name)
        } else {
            return;
        };
        let Some(result) = constant_result(&Operand::Name(lhs), op, &rhs) else { return };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, MSG, |c| c.replace(s, e, &result.to_string()));
    }
}
