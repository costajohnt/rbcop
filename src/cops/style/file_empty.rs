use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;
use helpers::select_by::plain_send;

pub const NAME: &str = "Style/FileEmpty";
pub static COP: FileEmpty = FileEmpty;
pub struct FileEmpty;

/// `(send $(const {nil? cbase} {:File :FileTest}) <methods> $_)`: the const and the argument.
fn file_call<'pr>(node: &Node<'pr>, methods: &[&str]) -> Option<(Node<'pr>, Node<'pr>)> {
    let c = plain_send(node)?;
    if !methods.contains(&method_name(&c)) {
        return None;
    }
    let recv = c.receiver()?;
    if !(is_top_const(&recv, "File") || is_top_const(&recv, "FileTest")) {
        return None;
    }
    let mut args = send_args(&c);
    if args.len() != 1 {
        return None;
    }
    Some((recv, args.pop().unwrap()))
}

/// The receiver of a `!` send.
fn bang_receiver<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let c = plain_send(node)?;
    (method_name(&c) == "!" && send_args(&c).is_empty()).then(|| c.receiver())?
}

/// `methods` on `node`, or on the receiver of `!node`.
fn maybe_banged<'pr>(node: &Node<'pr>, methods: &[&str]) -> Option<(Node<'pr>, Node<'pr>)> {
    match bang_receiver(node) {
        Some(r) => file_call(&r, methods),
        None => file_call(node, methods),
    }
}

fn is_empty_str(node: &Node) -> bool {
    node.as_string_node().is_some_and(|s| s.unescaped().is_empty()) && !is_parser_dstr(node)
}

impl Cop for FileEmpty {
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
        let Some(call) = plain_send(node) else { return };
        let name = method_name(&call);
        if !matches!(name, ">=" | "!=" | "==" | "zero?" | "empty?") {
            return;
        }
        let args = send_args(&call);
        let Some(recv) = call.receiver() else { return };
        let found = match (name, args.as_slice()) {
            ("zero?", [_]) => file_call(node, &["zero?"]),
            ("==" | ">=", [rhs]) if int_literal_value(rhs) == Some(0) => maybe_banged(&recv, &["size"]),
            ("==" | "!=", [rhs]) if is_empty_str(rhs) => maybe_banged(&recv, &["read", "binread"]),
            ("zero?", []) => file_call(&recv, &["size"]),
            ("empty?", []) => file_call(&recv, &["read", "binread"]),
            _ => None,
        };
        let Some((const_node, arg)) = found else { return };
        let receiver_bang = bang_receiver(&recv).is_some();
        let bang = (name == "==" && receiver_bang) || (matches!(name, ">=" | "!=") && !receiver_bang);
        let (cls, arg) = (node_text(&const_node), node_text(&arg));
        let msg = format!("Use `{cls}.empty?({arg})` instead.");
        let replacement = format!("{}{cls}.empty?({arg})", if bang { "!" } else { "" });
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
