use crate::prelude::*;

pub const NAME: &str = "Lint/UselessOr";
pub static COP: UselessOr = UselessOr;
pub struct UselessOr;

/// `(send _ %TRUTHY_RETURN_VALUE_METHODS)`: a plain call without arguments or block.
fn truthy_return_value_method(node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    !is_csend(&call)
        && call.arguments().is_none()
        && call.block().is_none()
        && matches!(
            method_name(&call),
            "to_a" | "to_c" | "to_d" | "to_i" | "to_f" | "to_h" | "to_r" | "to_s" | "to_sym" | "intern" | "inspect" | "hash" | "object_id" | "__id__"
        )
}

fn report(cx: &mut Cx, or: &ruby_prism::OrNode, truthy: &Node) {
    let (lhs, rhs) = (or.left(), or.right());
    let start = or.operator_loc().start_offset();
    let end = rhs.location().end_offset();
    let msg = format!(
        "`{}` will never evaluate because `{}` always returns a truthy value.",
        node_text(&rhs),
        node_text(truthy)
    );
    let loc = or.location();
    let (or_start, or_end) = (loc.start_offset(), loc.end_offset());
    let replacement = node_text(&lhs).to_string();
    cx.add_fix(start, end, msg, |c| c.replace(or_start, or_end, &replacement));
}

impl Cop for UselessOr {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::OR_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let or = node.as_or_node().unwrap();
        if truthy_return_value_method(&or.left()) {
            return report(cx, &or, &or.left());
        }
        if !truthy_return_value_method(&or.right()) {
            return;
        }
        // The parser parent, skipping one `begin` (parentheses).
        let n = cx.ancestors.len();
        let parent = match cx.ancestors.last().map(kind::of) {
            Some(kind::OR_NODE) => Some(n - 1),
            Some(kind::STATEMENTS_NODE) if n >= 3 && kind::of(&cx.ancestors[n - 2]) == kind::PARENTHESES_NODE => Some(n - 3),
            _ => None,
        };
        let Some(outer) = parent.and_then(|i| cx.ancestors[i].as_or_node()) else { return };
        report(cx, &outer, &or.right());
    }
}
