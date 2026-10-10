use crate::helpers::trailing_comma::{Subject, check, range};
use crate::prelude::*;

pub const NAME: &str = "Style/TrailingCommaInArguments";
pub static COP: TrailingCommaInArguments = TrailingCommaInArguments;
pub struct TrailingCommaInArguments;

/// `a[i,] op= v`: parser's op_asgn holds a `(send a :[] i)` that `on_send` visits.
macro_rules! index_write {
    ($node:expr, $($as:ident),*) => {
        $(if let Some(n) = $node.$as() {
            let mut args: Vec<Node> = n.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
            args.extend(n.block().map(|b| b.as_node()));
            let start = n.receiver().map_or(n.opening_loc().start_offset(), |r| r.location().start_offset());
            let sel = n.opening_loc().start_offset();
            let close = n.closing_loc();
            return Some((args, (start, close.end_offset()), sel, None));
        })*
    };
}

/// (arguments, send range, selector start, closing paren when parenthesized)
#[allow(clippy::type_complexity)]
fn parts<'pr>(node: &Node<'pr>) -> Option<(Vec<Node<'pr>>, (usize, usize), usize, Option<(usize, usize)>)> {
    if let Some(call) = node.as_call_node() {
        let paren = call.opening_loc().filter(|o| o.as_slice() == b"(");
        if paren.is_none() && method_name(&call) != "[]" {
            return None;
        }
        let range = send_range(&call);
        let sel = call.message_loc().map_or(range.0, |m| m.start_offset());
        let close = paren.and(call.closing_loc()).map(|c| (c.start_offset(), c.end_offset()));
        return Some((send_args(&call), range, sel, close));
    }
    index_write!(node, as_index_operator_write_node, as_index_or_write_node, as_index_and_write_node);
    None
}

impl Cop for TrailingCommaInArguments {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::INDEX_OPERATOR_WRITE_NODE, kind::INDEX_OR_WRITE_NODE, kind::INDEX_AND_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((args, send, sel, close)) = parts(node) else { return };
        let Some(last) = args.last() else { return };
        let last_range = range(last);
        let mut elements = Vec::new();
        for a in &args {
            match a.as_keyword_hash_node() {
                Some(h) if cx.src.line_of(range(a).0) != cx.src.line_of(range(a).1) => {
                    elements.extend(h.elements().iter().map(|e| range(&e)))
                }
                _ => elements.push(range(a)),
            }
        }
        let subject = Subject {
            range: send,
            end_loc: close.unwrap_or((send.1 - 1, send.1)),
            elements,
            call: Some((sel, last_range, last.as_hash_node().is_some())),
        };
        check(cx, &subject, &args, "parameter of {article} method call", last_range.1, send.1);
    }
}
