use crate::prelude::*;
use helpers::allowed_receivers::allowed_receiver;

pub const NAME: &str = "Style/HashLookupMethod";
pub static COP: HashLookupMethod = HashLookupMethod;
pub struct HashLookupMethod;

/// Ranges passed to `ignore_node` (first arguments of offending calls).
type Ignored = Vec<(usize, usize)>;

/// `ignore_node` extent of a node: a heredoc reaches its terminator line.
fn ignored_extent(node: &Node) -> (usize, usize) {
    let loc = node.location();
    let mut end = loc.end_offset();
    if is_heredoc(node) {
        let closing = match kind::of(node) {
            kind::STRING_NODE => node.as_string_node().unwrap().closing_loc(),
            kind::INTERPOLATED_STRING_NODE => node.as_interpolated_string_node().unwrap().closing_loc(),
            kind::X_STRING_NODE => Some(node.as_x_string_node().unwrap().closing_loc()),
            kind::INTERPOLATED_X_STRING_NODE => Some(node.as_interpolated_x_string_node().unwrap().closing_loc()),
            _ => None,
        };
        if let Some(c) = closing {
            let s = c.as_slice();
            end = c.end_offset() - usize::from(s.ends_with(b"\n")) - usize::from(s.ends_with(b"\r\n"));
        }
    }
    (loc.start_offset(), end)
}

impl Cop for HashLookupMethod {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let fetch_style = cx.cfg.str("EnforcedStyle") == Some("fetch");
        // (name, receiver, args, selector, dot, send range, csend, block literal)
        let (name, receiver, args, selector, dot, range, csend, block_literal) = if let Some(call) = node.as_call_node() {
            let name = method_name(&call);
            if name != "[]" && name != "fetch" {
                return;
            }
            let span = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
            (
                name,
                call.receiver(),
                send_args(&call),
                call.message_loc().map(span),
                call.call_operator_loc().map(span),
                send_range(&call),
                is_csend(&call),
                call.block().is_some_and(|b| b.as_block_node().is_some()),
            )
        } else {
            // `a[i] op= v`: parser's op_asgn holds a `send :[]`.
            if !fetch_style {
                return;
            }
            let parts = match node {
                Node::IndexOperatorWriteNode { .. } => {
                    let n = node.as_index_operator_write_node().unwrap();
                    (n.receiver(), n.arguments(), n.block(), n.opening_loc(), n.closing_loc())
                }
                Node::IndexOrWriteNode { .. } => {
                    let n = node.as_index_or_write_node().unwrap();
                    (n.receiver(), n.arguments(), n.block(), n.opening_loc(), n.closing_loc())
                }
                _ => {
                    let n = node.as_index_and_write_node().unwrap();
                    (n.receiver(), n.arguments(), n.block(), n.opening_loc(), n.closing_loc())
                }
            };
            let (receiver, arguments, block, open, close) = parts;
            let Some(receiver) = receiver else { return };
            let mut args: Vec<Node> = arguments.map(|a| a.arguments().iter().collect()).unwrap_or_default();
            args.extend(block.map(|b| b.as_node()));
            let start = receiver.location().start_offset();
            ("[]", Some(receiver), args, Some((open.start_offset(), close.end_offset())), None, (start, close.end_offset()), false, false)
        };
        if receiver.as_ref().is_some_and(|r| allowed_receiver(cx, r)) {
            return;
        }
        if cx.state::<Ignored>().iter().any(|&(s, e)| s <= range.0 && e >= range.1) {
            return;
        }
        let brackets = !fetch_style
            && receiver.is_some()
            && name == "fetch"
            && args.len() == 1
            && !block_literal
            && !csend;
        let fetch = fetch_style && name == "[]" && args.len() == 1;
        if !brackets && !fetch {
            return;
        }
        let key = node_text(&args[0]).to_string();
        let ignored = ignored_extent(&args[0]);
        cx.state::<Ignored>().push(ignored);
        let end = range.1;
        if brackets {
            let Some(sel) = selector else { return };
            let Some(dot) = dot else { return };
            cx.add_fix(sel.0, sel.1, "Use `Hash#[]` instead of `Hash#fetch`.", |c| {
                c.replace(dot.0, end, &format!("[{key}]"));
            });
        } else {
            let from = if csend { dot } else { selector };
            let Some(from) = from else { return };
            let text = if csend { format!("&.fetch({key})") } else { format!(".fetch({key})") };
            cx.add_fix(range.0, range.1, "Use `Hash#fetch` instead of `Hash#[]`.", |c| c.replace(from.0, end, &text));
        }
    }
}
