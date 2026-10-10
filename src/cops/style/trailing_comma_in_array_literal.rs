use crate::helpers::trailing_comma::{Subject, check};
use crate::prelude::*;

pub const NAME: &str = "Style/TrailingCommaInArrayLiteral";
pub static COP: TrailingCommaInArrayLiteral = TrailingCommaInArrayLiteral;
pub struct TrailingCommaInArrayLiteral;

impl Cop for TrailingCommaInArrayLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        // square_brackets? and check_literal
        let (Some(open), Some(close)) = (array.opening_loc(), array.closing_loc()) else { return };
        if open.as_slice() != b"[" {
            return;
        }
        let items: Vec<Node> = array.elements().iter().collect();
        let Some(last) = items.last() else { return };
        let begin = last.location().end_offset();
        let end_loc = (close.start_offset(), close.end_offset());
        let subject = Subject::literal(node, &items, end_loc);
        check(cx, &subject, &items, "item of {article} array", begin, end_loc.0);
    }
}
