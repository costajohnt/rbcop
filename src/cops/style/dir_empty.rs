use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/DirEmpty";
pub static COP: DirEmpty = DirEmpty;
pub struct DirEmpty;

/// A plain send (no csend, no block, no block pass) named `name` with `nargs` arguments.
fn plain_send<'pr>(node: &Node<'pr>, name: &str, nargs: usize) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    (!is_csend(&call) && call.block().is_none() && method_name(&call) == name && call_args(&call).len() == nargs)
        .then_some(call)
}

/// `(send $(const {nil? cbase} :Dir) name $_)`: (const, arg).
fn dir_call<'pr>(node: &Node<'pr>, name: &str) -> Option<(Node<'pr>, Node<'pr>)> {
    let call = plain_send(node, name, 1)?;
    let recv = call.receiver().filter(|r| is_top_const(r, "Dir"))?;
    Some((recv, call_args(&call).pop()?))
}

impl Cop for DirEmpty {
    fn correctable(&self) -> bool {
        true
    }
    fn ruby_range(&self) -> (f64, f64) {
        (2.4, f64::MAX)
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "==" | "!=" | ">" | "empty?" | "none?") || is_csend(&call) {
            return;
        }
        if call.block().is_some() {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = call_args(&call);
        let int_arg = match args.as_slice() {
            [a] => int_literal_value(a),
            _ => None,
        };
        let found = match name {
            "==" | "!=" | ">" => plain_send(&recv, "size", 0).and_then(|size| {
                let inner = size.receiver()?;
                match int_arg {
                    Some(2) => dir_call(&inner, "entries"),
                    Some(0) => dir_call(&inner, "children"),
                    _ => None,
                }
            }),
            "empty?" if args.is_empty() => dir_call(&recv, "children"),
            "none?" if args.is_empty() => dir_call(&recv, "each_child"),
            _ => None,
        };
        let Some((konst, arg)) = found else { return };
        let bang = if matches!(name, "!=" | ">") { "!" } else { "" };
        let replacement = format!("{bang}{}.empty?({})", node_text(&konst), node_text(&arg));
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, format!("Use `{replacement}` instead."), |c| c.replace(s, e, &replacement));
    }
}
