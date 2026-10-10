use crate::prelude::*;

pub const NAME: &str = "Style/UnlessElse";
pub static COP: UnlessElse = UnlessElse;
pub struct UnlessElse;

fn unless_else(node: &Node) -> bool {
    node.as_unless_node().is_some_and(|u| u.else_clause().is_some())
}

impl Cop for UnlessElse {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !unless_else(node) {
            return;
        }
        // Inside an already-flagged unless/else (ignore_node) the corrector stays empty.
        let correctable = !cx.ancestors.iter().any(unless_else);
        let loc = node.location();
        cx.add_with(
            loc.start_offset(),
            loc.end_offset(),
            "Do not use `unless` with `else`. Rewrite these with the positive case first.",
            correctable,
        );
    }
}
