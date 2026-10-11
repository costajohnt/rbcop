use crate::prelude::*;
use helpers::select_by::plain_send;

pub const NAME: &str = "Style/ExpandPathArguments";
pub static COP: ExpandPathArguments = ExpandPathArguments;
pub struct ExpandPathArguments;

const PATHNAME_MSG: &str = "Use `Pathname(__dir__).expand_path` instead of `Pathname(__FILE__).parent.expand_path`.";
const PATHNAME_NEW_MSG: &str =
    "Use `Pathname.new(__dir__).expand_path` instead of `Pathname.new(__FILE__).parent.expand_path`.";

/// (const {nil? cbase} :Name)
fn is_const(node: &Node, name: &str) -> bool {
    match node {
        Node::ConstantReadNode { .. } => node_text(node) == name,
        Node::ConstantPathNode { .. } => {
            let p = node.as_constant_path_node().unwrap();
            p.parent().is_none() && text(&p.name_loc()) == name
        }
        _ => false,
    }
}

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// Ruby `str.split('/')` (trailing empty fields dropped).
fn split(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

fn depth(path: &str) -> usize {
    split(path).iter().filter(|p| **p != ".").count()
}

fn parent_path(path: &str) -> String {
    let mut parts: Vec<&str> = split(path).into_iter().filter(|p| *p != ".").collect();
    if let Some(i) = parts.iter().position(|p| *p == "..") {
        parts.remove(i);
    }
    parts.join("/")
}

/// Drop the first and last characters (`strip_surrounded_quotes!`).
fn strip_quotes(s: &str) -> &str {
    let mut chars = s.char_indices();
    let start = chars.next().map_or(0, |(_, c)| c.len_utf8());
    let end = s.char_indices().next_back().map_or(0, |(i, _)| i);
    if start > end { "" } else { &s[start..end] }
}

impl Cop for ExpandPathArguments {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "expand_path" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = send_args(&call);
        if is_const(&recv, "File") {
            if args.len() != 2 {
                return;
            }
            let (current, default_dir) = (&args[0], &args[1]);
            if node_text(default_dir) != "__FILE__" || current.as_string_node().is_none() || is_parser_dstr(current) {
                return;
            }
            let path = strip_quotes(node_text(current));
            let parent = parent_path(path);
            let new_path = if parent.is_empty() { String::new() } else { format!("'{parent}', ") };
            let d = depth(path);
            let new_default = if d == 0 { "__FILE__" } else { "__dir__" };
            let msg = format!("Use `expand_path({new_path}{new_default})` instead of `expand_path('{path}', __FILE__)`.");
            let Some(sel) = call.message_loc() else { return };
            let (cur, dir) = (range(current), range(default_dir));
            cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| match d {
                0 => c.replace(cur.0, dir.1, "__FILE__"),
                1 => c.replace(cur.0, dir.1, "__dir__"),
                _ => {
                    c.replace(cur.0, cur.1, &format!("'{parent}'"));
                    c.replace(dir.0, dir.1, "__dir__");
                }
            });
            return;
        }
        // (send (send (send nil? :Pathname $_) :parent) :expand_path) and the Pathname.new variant.
        if !args.is_empty() {
            return;
        }
        let Some(parent) = plain_send(&recv) else { return };
        if method_name(&parent) != "parent" || !send_args(&parent).is_empty() {
            return;
        }
        let Some(inner) = parent.receiver().as_ref().and_then(plain_send) else { return };
        let inner_args = send_args(&inner);
        if inner_args.len() != 1 {
            return;
        }
        let msg = match inner.receiver() {
            None if method_name(&inner) == "Pathname" => PATHNAME_MSG,
            Some(r) if method_name(&inner) == "new" && is_const(&r, "Pathname") => PATHNAME_NEW_MSG,
            _ => return,
        };
        let dir = &inner_args[0];
        if node_text(dir) != "__FILE__" {
            return;
        }
        let dir = range(dir);
        let (Some(dot), Some(sel)) = (parent.call_operator_loc(), parent.message_loc()) else { return };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, msg, |c| {
            c.replace(dir.0, dir.1, "__dir__");
            c.remove(dot.start_offset(), dot.end_offset());
            c.remove(sel.start_offset(), sel.end_offset());
        });
    }
}
