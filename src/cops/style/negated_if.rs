use crate::prelude::*;

pub const NAME: &str = "Style/NegatedIf";
pub static COP: NegatedIf = NegatedIf;
pub struct NegatedIf;

impl Cop for NegatedIf {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let n = node.as_if_node().unwrap();
        // ternary, elsif, `in pat if cond`
        let Some(kw) = n.if_keyword_loc() else { return };
        if text(&kw) != "if" || is_pattern_guard(node, cx.ancestors) || n.subsequent().is_some() {
            return;
        }
        let modifier = n.end_keyword_loc().is_none();
        match cx.cfg.str("EnforcedStyle") {
            Some("prefix") if modifier => return,
            Some("postfix") if !modifier => return,
            _ => {}
        }
        if helpers::negative_conditional::single_negative(n.predicate()) {
            let loc = node.location();
            let keyword = (kw.start_offset(), kw.end_offset());
            cx.add_fix(loc.start_offset(), loc.end_offset(), helpers::negative_conditional::message("unless", "if"), |c| {
                helpers::negative_conditional::correct_negative_condition(c, keyword, "unless", n.predicate())
            });
        }
    }
}
