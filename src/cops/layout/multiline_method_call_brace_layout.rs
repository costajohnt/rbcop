use crate::helpers::multiline_literal_brace_layout as mlbl;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineMethodCallBraceLayout";
pub static COP: MultilineMethodCallBraceLayout = MultilineMethodCallBraceLayout;
pub struct MultilineMethodCallBraceLayout;

const MSGS: mlbl::Messages = [
    "Closing method call brace must be on the same line as the last argument when opening brace is on the same line as the first argument.",
    "Closing method call brace must be on the line after the last argument when opening brace is on a separate line from the first argument.",
    "Closing method call brace must be on the line after the last argument.",
    "Closing method call brace must be on the same line as the last argument.",
];

impl Cop for MultilineMethodCallBraceLayout {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // Parser gives `a[1]` no loc.begin; only real parentheses count.
        let (Some(open), Some(close)) = (call.opening_loc(), call.closing_loc()) else { return };
        if open.as_slice() != b"(" || cx.src.line_of(open.start_offset()) == cx.src.line_of(close.start_offset()) {
            return;
        }
        let children = send_args(&call);
        let range = send_range(&call);
        let owns_block = call.block().is_some_and(|b| kind::of(&b) == kind::BLOCK_NODE);
        let ca = !owns_block && mlbl::chained_or_argument(node, cx.ancestors);
        let close = (close.start_offset(), close.end_offset());
        let chain = if owns_block || !first_is_str_heredoc(&children) { Ok(None) } else { heredoc_chain(node, cx.ancestors) };
        mlbl::check(cx, &MSGS, open.start_offset(), close, range, &children, ca, chain);
    }
}

/// `first_argument.str_type? && first_argument.heredoc?`: parser makes a heredoc a `str` only
/// when its body has at most one line and no interpolation.
fn first_is_str_heredoc(args: &[Node]) -> bool {
    args.first().and_then(|a| a.as_string_node()).is_some_and(|s| {
        s.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"<<"))
            && s.content_loc().as_slice().iter().filter(|&&b| b == b'\n').count() <= 1
    })
}

/// The parent call's `.method...` range (`loc.dot` to its end); Err when it has no dot.
fn heredoc_chain(node: &Node, ancestors: &[Node]) -> Result<Option<(usize, usize)>, ()> {
    let n = ancestors.len();
    let me = node.location().start_offset();
    let parent = match ancestors.last() {
        Some(p) if p.as_call_node().is_some_and(|c| c.receiver().is_some_and(|r| r.location().start_offset() == me)) => {
            p.as_call_node()
        }
        Some(p) if kind::of(p) == kind::ARGUMENTS_NODE && n >= 2 => ancestors[n - 2].as_call_node(),
        _ => None,
    };
    let Some(parent) = parent else { return Ok(None) };
    let dot = parent.call_operator_loc().ok_or(())?;
    Ok(Some((dot.start_offset(), send_range(&parent).1)))
}
