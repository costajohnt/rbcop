use crate::ast::{is_heredoc, send_args, send_range};
use crate::helpers::heredoc;
use crate::prelude::*;

pub const NAME: &str = "Lint/HeredocMethodCallPosition";
pub static COP: HeredocMethodCallPosition = HeredocMethodCallPosition;
pub struct HeredocMethodCallPosition;

const MSG: &str = "Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.";

/// A parser `send`/`csend` (a Prism call owning no `do`/`{}` block, which would be a parser `block`).
fn send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
}

use helpers::ruby_string::strip;

impl Cop for HeredocMethodCallPosition {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }
    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // heredoc_node_descendent_receiver: the chain of sends down to the heredoc receiver.
        let mut chain = vec![call];
        let heredoc_end = loop {
            let Some(recv) = chain.last().unwrap().receiver() else { return };
            if is_heredoc(&recv) {
                let Some((_, (_, end), _)) = heredoc::parts(&recv) else { return };
                break end;
            }
            let Some(next) = send(&recv) else { return };
            chain.push(next);
        };
        let src = cx.src;
        let (call_start, call_end) = send_range(&chain[0]);
        if heredoc_end > call_end {
            return;
        }
        let text = src.text;
        let start = heredoc_end + 1;
        let end = start + text.get(start..).and_then(|t| t.chars().next()).map_or(1, char::len_utf8);

        // calls_on_multiple_lines?
        let last_line = src.line_of(call_end);
        let multiple = chain.iter().any(|c| {
            let args = send_args(c);
            src.line_of(send_range(c).1) != last_line
                || args.first().is_some_and(|f| {
                    src.line_of(f.location().start_offset()) != src.line_of(args.last().unwrap().location().end_offset())
                })
        });
        let line_range = |pos: usize| {
            let s = text[..pos].rfind('\n').map_or(0, |i| i + 1);
            let e = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
            (s, e)
        };
        // call_range_to_safely_reposition
        let call_range = (!multiple && heredoc_end <= call_end).then(|| {
            let call_source = strip(&text[heredoc_end..call_end]);
            let (ls, le) = line_range(call_end);
            let line_source = strip(&text[ls..le]);
            if call_source == line_source {
                Some((heredoc_end, call_end))
            } else if format!("{call_source},") == line_source {
                Some((heredoc_end, call_end + 1))
            } else {
                None
            }
        });
        let begin_line = line_range(call_start);
        cx.add_fix(start, end, MSG, |c| {
            let Some(Some((s, e))) = call_range else { return };
            c.remove(s, e);
            c.insert_after(begin_line.0, begin_line.1, strip(&text[s..e]));
        });
    }
}
