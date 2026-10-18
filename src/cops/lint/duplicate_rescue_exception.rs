use crate::helpers::node_equality;
use crate::prelude::*;

pub const NAME: &str = "Lint/DuplicateRescueException";
pub static COP: DuplicateRescueException = DuplicateRescueException;
pub struct DuplicateRescueException;

impl Cop for DuplicateRescueException {
    fn kinds(&self) -> &'static [usize] {
        &[kind::BEGIN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let mut clause = node.as_begin_node().unwrap().rescue_clause();
        let mut previous: Vec<(usize, usize)> = Vec::new();
        while let Some(r) = clause {
            for e in r.exceptions().iter() {
                let l = e.location();
                let range = (l.start_offset(), l.end_offset());
                if previous.iter().any(|&p| node_equality::same_hash(cx.src, p, range)) {
                    cx.add(range.0, range.1, "Duplicate `rescue` exception detected.");
                } else {
                    previous.push(range);
                }
            }
            clause = r.subsequent();
        }
    }
}
