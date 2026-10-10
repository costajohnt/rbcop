use crate::helpers::trailing_comma::{Subject, check};
use crate::prelude::*;

pub const NAME: &str = "Style/TrailingCommaInHashLiteral";
pub static COP: TrailingCommaInHashLiteral = TrailingCommaInHashLiteral;
pub struct TrailingCommaInHashLiteral;

impl Cop for TrailingCommaInHashLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let hash = node.as_hash_node().unwrap();
        let items: Vec<Node> = hash.elements().iter().collect();
        let Some(last) = items.last() else { return };
        let begin = last.location().end_offset();
        let close = hash.closing_loc();
        let end_loc = (close.start_offset(), close.end_offset());
        let subject = Subject::literal(node, &items, end_loc);
        check(cx, &subject, &items, "item of {article} hash", begin, end_loc.0);
    }
}
