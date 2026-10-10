use crate::prelude::*;
use ruby_prism::CallNode;
use std::collections::HashSet;

pub const NAME: &str = "Style/DigChain";
pub static COP: DigChain = DigChain;
pub struct DigChain;

/// DigHelp `dig?`: `(call _ :dig !{hash block_pass}+)`: the parser arguments.
fn dig<'pr>(node: &Node<'pr>) -> Option<(CallNode<'pr>, Vec<Node<'pr>>)> {
    let call = node.as_call_node()?;
    if method_name(&call) != "dig" {
        return None;
    }
    let args = send_args(&call);
    let bad = |n: &Node| {
        matches!(kind::of(n), kind::HASH_NODE | kind::KEYWORD_HASH_NODE | kind::BLOCK_ARGUMENT_NODE)
    };
    (!args.is_empty() && !args.iter().any(bad)).then_some((call, args))
}

impl Cop for DigChain {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "dig" {
            return;
        }
        let (s, e) = send_range(&call);
        if cx.state::<HashSet<(usize, usize)>>().contains(&(s, e)) {
            return;
        }
        if call.call_operator_loc().is_none() {
            return;
        }
        let Some((_, mut arguments)) = dig(node) else { return };
        let mut begin = None;
        let mut cur = call.receiver();
        while let Some(r) = cur {
            let Some((inner, args)) = dig(&r) else { break };
            begin = inner.message_loc().map(|l| l.start_offset());
            arguments.splice(0..0, args);
            cx.state::<HashSet<(usize, usize)>>().insert(send_range(&inner));
            cur = inner.receiver();
        }
        // `invalid_arguments?`: `...` anywhere but last.
        if arguments.iter().position(|a| a.as_forwarding_arguments_node().is_some()).is_some_and(|i| i < arguments.len() - 1) {
            return;
        }
        let Some(begin) = begin else { return };
        let replacement = format!("dig({})", arguments.iter().map(|a| node_text(a)).collect::<Vec<_>>().join(", "));
        let end_line = helpers::comments_help::find_end_line(cx, node);
        let comments = helpers::comments_help::comments_in_lines(cx, cx.src.line_of(s), end_line);
        let src = cx.src;
        let msg = format!("Use `{replacement}` instead of chaining.");
        cx.add_fix(begin, e, msg, |c| {
            c.replace(begin, e, &replacement);
            for comment in comments.iter().rev() {
                if comment.start >= e {
                    continue;
                }
                c.insert_before(s, e, &format!("{}\n", src.slice(comment.start, comment.end)));
            }
        });
    }
}
