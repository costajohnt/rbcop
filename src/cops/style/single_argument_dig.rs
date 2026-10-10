use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/SingleArgumentDig";
pub static COP: SingleArgumentDig = SingleArgumentDig;
pub struct SingleArgumentDig;

/// DigHelp `dig?`: `(call _ :dig !{hash block_pass}+)` (a call with an attached block is a parser
/// `block`, not a `call`).
fn is_dig(node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if method_name(&call) != "dig" || call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    let args = send_args(&call);
    !args.is_empty()
        && args.iter().all(|a| {
            !matches!(kind::of(a), kind::HASH_NODE | kind::KEYWORD_HASH_NODE | kind::BLOCK_ARGUMENT_NODE)
        })
}

fn is_dig_call(call: &CallNode) -> bool {
    is_dig(&call.as_node())
}

impl Cop for SingleArgumentDig {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "dig" || is_csend(&call) {
            return;
        }
        let Some(receiver) = call.receiver() else { return };
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        if matches!(
            kind::of(arg),
            kind::SPLAT_NODE
                | kind::BLOCK_ARGUMENT_NODE
                | kind::FORWARDING_ARGUMENTS_NODE
                | kind::HASH_NODE
                | kind::KEYWORD_HASH_NODE
        ) {
            return;
        }
        let parent_dig = if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            false
        } else {
            helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(is_dig)
        };
        if helpers::alignment::cop_enabled(cx, "Style/DigChain")
            && (receiver.as_call_node().is_some_and(|r| is_dig_call(&r)) || parent_dig)
        {
            return;
        }
        let (start, end) = send_range(&call);
        let receiver = node_text(&receiver);
        let argument = node_text(arg);
        let original = &cx.src.text[start..end];
        let msg = format!("Use `{receiver}[{argument}]` instead of `{original}`.");
        let ignored = cx.state::<Vec<(usize, usize)>>().iter().any(|&(a, b)| a <= start && end <= b);
        let replacement = format!("{receiver}[{argument}]");
        cx.add_fix(start, end, msg, |c| {
            if !ignored {
                c.replace(start, end, &replacement);
            }
        });
        cx.state::<Vec<(usize, usize)>>().push((start, end));
    }
}
