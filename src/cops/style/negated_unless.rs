use crate::prelude::*;

pub const NAME: &str = "Style/NegatedUnless";
pub static COP: NegatedUnless = NegatedUnless;
pub struct NegatedUnless;

impl Cop for NegatedUnless {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let n = node.as_unless_node().unwrap();
        if is_pattern_guard(node, cx.ancestors) || n.else_clause().is_some() {
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
            let kw = n.keyword_loc();
            let keyword = (kw.start_offset(), kw.end_offset());
            cx.add_fix(loc.start_offset(), loc.end_offset(), helpers::negative_conditional::message("if", "unless"), |c| {
                helpers::negative_conditional::correct_negative_condition(c, keyword, "if", n.predicate())
            });
        }
    }
}
