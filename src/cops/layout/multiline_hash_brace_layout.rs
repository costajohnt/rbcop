use crate::helpers::multiline_literal_brace_layout as mlbl;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineHashBraceLayout";
pub static COP: MultilineHashBraceLayout = MultilineHashBraceLayout;
pub struct MultilineHashBraceLayout;

const MSGS: mlbl::Messages = [
    "Closing hash brace must be on the same line as the last hash element when opening brace is on the same line as the first hash element.",
    "Closing hash brace must be on the line after the last hash element when opening brace is on a separate line from the first hash element.",
    "Closing hash brace must be on the line after the last hash element.",
    "Closing hash brace must be on the same line as the last hash element.",
];

impl Cop for MultilineHashBraceLayout {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let h = node.as_hash_node().unwrap();
        let (open, close) = (h.opening_loc(), h.closing_loc());
        let children: Vec<Node> = h.elements().iter().collect();
        let loc = node.location();
        let ca = mlbl::chained_or_argument(node, cx.ancestors);
        let close = (close.start_offset(), close.end_offset());
        mlbl::check(cx, &MSGS, open.start_offset(), close, (loc.start_offset(), loc.end_offset()), &children, ca);
    }
}
