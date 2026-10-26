use crate::helpers::statement_modifier::{self as sm, Stmt};
use crate::prelude::*;

pub const NAME: &str = "Style/WhileUntilModifier";
pub static COP: WhileUntilModifier = WhileUntilModifier;
pub struct WhileUntilModifier;

impl Cop for WhileUntilModifier {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let post = match node {
            Node::WhileNode { .. } => node.as_while_node().unwrap().is_begin_modifier(),
            _ => node.as_until_node().unwrap().is_begin_modifier(),
        };
        if post {
            return;
        }
        let Some(s) = Stmt::of(node) else { return };
        // non_eligible_body?: a conditional or `begin` body, then the base checks.
        let conditional_body = s.body.as_ref().is_some_and(|b| {
            b.as_begin_node().is_some_and(|k| k.begin_keyword_loc().is_some())
                || matches!(
                b,
                Node::IfNode { .. }
                    | Node::UnlessNode { .. }
                    | Node::WhileNode { .. }
                    | Node::UntilNode { .. }
                    | Node::CaseNode { .. }
                    | Node::CaseMatchNode { .. }
            ) && !b.as_while_node().is_some_and(|w| w.is_begin_modifier())
                && !b.as_until_node().is_some_and(|w| w.is_begin_modifier())
        });
        let parenthesize = sm::parenthesize(cx.ancestors);
        if sm::non_eligible_node(cx, &s)
            || conditional_body
            || sm::non_eligible_body(cx, s.body.as_ref())
            || sm::non_eligible_condition(&s.condition)
            || !sm::modifier_fits_on_single_line(cx, &s, parenthesize)
        {
            return;
        }
        let replacement = sm::to_modifier_form(cx, &s, parenthesize);
        let (start, end) = s.range;
        let msg = format!("Favor modifier `{}` usage when having a single-line body.", s.keyword);
        cx.add_fix(s.keyword_loc.0, s.keyword_loc.1, msg, |c| c.replace(start, end, &replacement));
    }
}
