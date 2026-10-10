use crate::prelude::*;

pub const NAME: &str = "Lint/DuplicateElsifCondition";
pub static COP: DuplicateElsifCondition = DuplicateElsifCondition;
pub struct DuplicateElsifCondition;

impl Cop for DuplicateElsifCondition {
    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let mut previous: Vec<Vec<String>> = Vec::new();
        let mut current = node.as_if_node().unwrap();
        // while node.if? || node.elsif?  (not ternaries)
        while current.if_keyword_loc().is_some_and(|k| matches!(k.as_slice(), b"if" | b"elsif")) {
            let loc = current.predicate().location();
            let shape = helpers::node_equality::shape(cx.src, loc.start_offset(), loc.end_offset());
            if previous.contains(&shape) {
                cx.add(loc.start_offset(), loc.end_offset(), "Duplicate `elsif` condition detected.");
            }
            previous.push(shape);
            // node = node.else_branch; break unless node&.if_type?
            let next = match current.subsequent() {
                Some(n) if n.as_if_node().is_some() => n.as_if_node(),
                Some(n) => n.as_else_node().and_then(|e| e.statements()).and_then(|s| {
                    let body: Vec<_> = s.body().iter().collect();
                    match body.as_slice() {
                        [only] => only.as_if_node(),
                        _ => None,
                    }
                }),
                None => None,
            };
            let Some(next) = next else { break };
            current = next;
        }
    }
}
