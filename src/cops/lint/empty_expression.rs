use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyExpression";
pub static COP: EmptyExpression = EmptyExpression;
pub struct EmptyExpression;

impl Cop for EmptyExpression {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PARENTHESES_NODE, kind::EMBEDDED_STATEMENTS_NODE]
    }

    // on_begin with no children: `()` and an empty interpolation `#{}`.
    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let empty = match node.as_parentheses_node() {
            Some(p) => p.body().is_none(),
            None => node.as_embedded_statements_node().unwrap().statements().is_none(),
        };
        if empty {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), "Avoid empty expressions.");
        }
    }
}
