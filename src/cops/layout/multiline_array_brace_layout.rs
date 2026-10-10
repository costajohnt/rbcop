use crate::helpers::multiline_literal_brace_layout as mlbl;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineArrayBraceLayout";
pub static COP: MultilineArrayBraceLayout = MultilineArrayBraceLayout;
pub struct MultilineArrayBraceLayout;

const MSGS: mlbl::Messages = [
    "The closing array brace must be on the same line as the last array element when the opening brace is on the same line as the first array element.",
    "The closing array brace must be on the line after the last array element when the opening brace is on a separate line from the first array element.",
    "The closing array brace must be on the line after the last array element.",
    "The closing array brace must be on the same line as the last array element.",
];

impl Cop for MultilineArrayBraceLayout {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let a = node.as_array_node().unwrap();
        let (Some(open), Some(close)) = (a.opening_loc(), a.closing_loc()) else { return };
        let children: Vec<Node> = a.elements().iter().collect();
        let loc = node.location();
        let ca = mlbl::chained_or_argument(node, cx.ancestors);
        let close = (close.start_offset(), close.end_offset());
        mlbl::check(cx, &MSGS, open.start_offset(), close, (loc.start_offset(), loc.end_offset()), &children, ca);
    }
}
