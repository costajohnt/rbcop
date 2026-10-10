use crate::helpers::node_types::is_operator_method;
use crate::prelude::*;

pub const NAME: &str = "Style/EvenOdd";
pub static COP: EvenOdd = EvenOdd;
pub struct EvenOdd;

fn int_is(node: &Node, want: i32) -> bool {
    node.as_integer_node().is_some_and(|i| {
        let v: Result<i32, _> = i.value().try_into();
        v.is_ok_and(|v| v == want)
    })
}

/// A parser `send` (not csend, not wrapped by a block).
fn plain_send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && !c.block().is_some_and(|b| b.as_block_node().is_some()))
}

/// `(send $_ :% (int 2))`
fn modulo_two<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = plain_send(node)?;
    let args = send_args(&call);
    if method_name(&call) != "%" || args.len() != 1 || !int_is(&args[0], 2) {
        return None;
    }
    call.receiver()
}

impl Cop for EvenOdd {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if is_csend(&call) || !matches!(method, "==" | "!=") {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = send_args(&call);
        if args.len() != 1 {
            return;
        }
        let arg = if int_is(&args[0], 0) {
            0
        } else if int_is(&args[0], 1) {
            1
        } else {
            return;
        };
        // {(send $_ :% (int 2)) (begin (send $_ :% (int 2)))}
        let inner = match recv.as_parentheses_node() {
            Some(p) => {
                let Some(body) = p.body() else { return };
                let mut stmts: Vec<Node> = match body.as_statements_node() {
                    Some(s) => s.body().iter().collect(),
                    None => vec![body],
                };
                if stmts.len() != 1 {
                    return;
                }
                stmts.pop().unwrap()
            }
            None => recv,
        };
        let Some(base) = modulo_two(&inner) else { return };
        let replacement = match (arg, method) {
            (0, "==") | (1, "!=") => "even",
            _ => "odd",
        };
        let base_src = node_text(&base);
        let base_src = match plain_send(&base) {
            Some(b) if is_operator_method(method_name(&b)) && method_name(&b) != "[]" => format!("({base_src})"),
            _ => base_src.to_string(),
        };
        let (s, e) = send_range(&call);
        let msg = format!("Replace with `Integer#{replacement}?`.");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &format!("{base_src}.{replacement}?")));
    }
}
