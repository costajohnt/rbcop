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
        let u = node.as_unless_node().unwrap();
        let kw = u.keyword_loc();
        let else_kw = u.else_clause().unwrap().else_keyword_loc();
        let Some(end_kw) = u.end_keyword_loc() else { return };
        // range_between_condition_and_else / range_between_else_and_end
        // Parser's `loc.begin` is `then`, or else a `;` ending the condition (Prism has no loc for it).
        let pred_end = u.predicate().location().end_offset();
        let semi = cx.src.text[pred_end..].trim_start_matches([' ', '\t']).starts_with(';');
        let body_start = match u.then_keyword_loc() {
            Some(t) => t.end_offset(),
            None if semi => cx.src.text[pred_end..].find(';').unwrap() + pred_end + 1,
            None => pred_end,
        };
        let body = (body_start, else_kw.start_offset());
        let else_range = (else_kw.end_offset(), end_kw.start_offset());
        cx.add_fix(
            loc.start_offset(),
            loc.end_offset(),
            "Do not use `unless` with `else`. Rewrite these with the positive case first.",
            |c| {
                if correctable {
                    c.replace(kw.start_offset(), kw.end_offset(), "if");
                    c.swap(body, else_range);
                }
            },
        );
    }
}
