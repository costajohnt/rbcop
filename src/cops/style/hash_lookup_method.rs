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
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let fetch_style = cx.cfg.str("EnforcedStyle") == Some("fetch");
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if name != "[]" && name != "fetch" {
            return;
        }
        // compound_assignment_target?: `a[i] op= v` (parser op_asgn/or_asgn/and_asgn around a
        // `send :[]`) is a Prism Index*WriteNode, never a CallNode, so it is never visited.
        let span = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
        let receiver = call.receiver();
        let args = send_args(&call);
        let selector = call.message_loc().map(span);
        let dot = call.call_operator_loc().map(span);
        let range = send_range(&call);
        let csend = is_csend(&call);
        let block_literal = call.block().is_some_and(|b| b.as_block_node().is_some());
        if receiver.as_ref().is_some_and(|r| allowed_receiver(cx, r)) {
            return;
        }
        if cx.state::<Ignored>().iter().any(|&(s, e)| s <= range.0 && e >= range.1) {
            return;
        }
        let brackets = !fetch_style
            && name == "fetch"
            && args.len() == 1
            && !block_literal
            && !csend
            && receiver.as_ref().is_some_and(|r| !exempt_from_brackets(cx, r, dot));
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
            let recv_end = receiver.unwrap().location().end_offset();
            cx.add_fix(sel.0, sel.1, "Use `Hash#[]` instead of `Hash#fetch`.", |c| {
                c.replace(recv_end, end, &format!("[{key}]"));
            });
        } else {
            let Some(from) = selector else { return };
            let text = if dot.is_some() { format!("fetch({key})") } else { format!(".fetch({key})") };
            cx.add_fix(range.0, range.1, "Use `Hash#fetch` instead of `Hash#[]`.", |c| c.replace(from.0, end, &text));
        }
    }
}

/// exempt_from_brackets?: a comment on the lines from the receiver's end to the dot, or
/// `ENV.fetch` while Style/FetchEnvVar is enabled.
fn exempt_from_brackets(cx: &Cx, receiver: &Node, dot: Option<(usize, usize)>) -> bool {
    let Some(dot) = dot else { return false };
    let (first, last) = (cx.src.line_of(receiver.location().end_offset()), cx.src.line_of(dot.1));
    helpers::comments_help::contains_comments(cx, first, last + 1)
        || (helpers::parser_parent::is_top_const(receiver, "ENV") && cx.config.cop_enabled("Style/FetchEnvVar"))
}
