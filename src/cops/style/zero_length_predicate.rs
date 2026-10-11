use crate::prelude::*;
use helpers::select_by::plain_send;
use helpers::parser_parent::{is_top_const, parser_parent};
use ruby_prism::CallNode;

pub const NAME: &str = "Style/ZeroLengthPredicate";
pub static COP: ZeroLengthPredicate = ZeroLengthPredicate;
pub struct ZeroLengthPredicate;

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// `(int n)` with `n` 0 or 1.
fn int_is(node: &Node, v: i32) -> bool {
    node.as_integer_node().and_then(|i| i.value().try_into().ok()) == Some(v)
}

/// Parser `send` arguments (block-pass included).
fn args<'pr>(call: &CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut a = call_args(call);
    a.extend(call.block().filter(|b| b.as_block_argument_node().is_some()));
    a
}

/// ZeroLengthPredicate#non_polymorphic_collection? on the parent send.
fn non_polymorphic(parent: &CallNode) -> bool {
    if is_csend(parent) {
        return false;
    }
    let Some(mid) = parent.receiver().and_then(|r| plain_send(&r)) else { return false };
    let Some(inner) = mid.receiver().and_then(|r| plain_send(&r)) else { return false };
    let Some(recv) = inner.receiver() else { return false };
    let name = method_name(&inner);
    let nargs = args(&inner).len();
    if is_top_const(&recv, "File") && name == "stat" && nargs == 1 {
        return true;
    }
    if ["File", "Tempfile", "StringIO"].iter().any(|c| is_top_const(&recv, c)) && (name == "new" || name == "open") {
        return true;
    }
    // (const (const {nil? cbase} :File) :Stat)
    name == "new"
        && recv.as_constant_path_node().is_some_and(|p| {
            p.name().is_some_and(|n| n.as_slice() == b"Stat") && p.parent().is_some_and(|q| is_top_const(&q, "File"))
        })
}

impl Cop for ZeroLengthPredicate {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if name != "size" && name != "length" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        if call.block().is_some() || !call_args(&call).is_empty() {
            return;
        }
        let Some(parent) = parser_parent(cx.ancestors).and_then(|p| p.as_call_node()) else { return };
        let me = span(node);
        let pargs = args(&parent);
        let is_recv = parent.receiver().is_some_and(|r| span(&r) == me);
        let is_arg = pargs.len() == 1 && span(&pargs[0]) == me;
        let op = method_name(&parent);
        let send_end = send_range(&parent).1;
        let src = cx.src;

        // check_zero_length_predicate
        if is_recv && op == "zero?" && pargs.is_empty() && !non_polymorphic(&parent) {
            let sel = call.message_loc().unwrap().start_offset();
            let msg = format!("Use `empty?` instead of `{}`.", src.slice(sel, send_end));
            cx.add_fix(sel, send_end, msg, |c| c.replace(sel, send_end, "empty?"));
            return;
        }

        let lhs_zero = |v: i32| is_recv && pargs.len() == 1 && int_is(&pargs[0], v);
        let rhs_zero = |v: i32| is_arg && parent.receiver().is_some_and(|r| int_is(&r, v));
        let (zero, current) = if lhs_zero(0) && op == "==" || lhs_zero(1) && op == "<" {
            (true, format!("{name} {op} {}", if op == "==" { 0 } else { 1 }))
        } else if rhs_zero(0) && op == "==" || rhs_zero(1) && op == ">" {
            (true, format!("{} {op} {name}", if op == "==" { 0 } else { 1 }))
        } else if !is_csend(&call) && lhs_zero(0) && (op == ">" || op == "!=") {
            (false, format!("{name} {op} 0"))
        } else if !is_csend(&call) && rhs_zero(0) && (op == "<" || op == "!=") {
            (false, format!("0 {op} {name}"))
        } else {
            return;
        };
        if non_polymorphic(&parent) {
            return;
        }
        let dot = call.call_operator_loc().map(|d| text(&d)).unwrap_or("");
        let bang = if zero && !is_csend(&parent) { "" } else { "!" };
        let replacement = format!("{bang}{}{dot}empty?", node_text(&recv));
        let msg = if zero {
            format!("Use `empty?` instead of `{current}`.")
        } else {
            format!("Use `!empty?` instead of `{current}`.")
        };
        let start = parent.location().start_offset();
        cx.add_fix(start, send_end, msg, |c| c.replace(start, send_end, &replacement));
    }
}
