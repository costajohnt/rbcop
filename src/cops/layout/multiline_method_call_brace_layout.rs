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
        mlbl::check(cx, &MSGS, open.start_offset(), close, range, &children, ca);
    }
}
