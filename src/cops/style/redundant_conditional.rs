use crate::prelude::*;
use ruby_prism::StatementsNode;

pub const NAME: &str = "Style/RedundantConditional";
pub static COP: RedundantConditional = RedundantConditional;
pub struct RedundantConditional;

const COMPARISON: &[&str] = &["==", "===", "!=", "<=", ">=", ">", "<"];

/// A branch that is exactly one `true` (Some(true)) or `false` (Some(false)) literal.
fn bool_branch(stmts: Option<StatementsNode>) -> Option<bool> {
    let s = stmts?;
    let mut it = s.body().iter();
    let only = it.next()?;
    if it.next().is_some() {
        return None;
    }
    if only.as_true_node().is_some() {
        Some(true)
    } else if only.as_false_node().is_some() {
        Some(false)
    } else {
        None
    }
}

impl Cop for RedundantConditional {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        // Parser children (condition, if_branch, else_branch); `unless` swaps the branches.
        let (cond, first, second, elsif, modifier, col_off) = if let Some(n) = node.as_if_node() {
            let else_stmts = n.subsequent().and_then(|s| s.as_else_node()).map(|e| e.statements());
            let Some(else_stmts) = else_stmts else { return };
            let elsif = n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif");
            let modifier = n.if_keyword_loc().is_some() && n.end_keyword_loc().is_none() && !elsif;
            (n.predicate(), n.statements(), else_stmts, elsif, modifier, n.location().start_offset())
        } else {
            let n = node.as_unless_node().unwrap();
            let Some(e) = n.else_clause() else { return };
            let modifier = n.end_keyword_loc().is_none();
            (n.predicate(), e.statements(), n.statements(), false, modifier, n.location().start_offset())
        };
        if modifier {
            return;
        }
        let Some(call) = cond.as_call_node() else { return };
        if is_csend(&call)
            || !COMPARISON.contains(&method_name(&call))
            || call.block().is_some()
            || call_args(&call).len() != 1
        {
            return;
        }
        let inverted = match (bool_branch(first), bool_branch(second)) {
            (Some(true), Some(false)) => false,
            (Some(false), Some(true)) => true,
            _ => return,
        };
        let src = cx.src;
        let condition = node_text(&cond);
        let expression = if inverted { format!("!({condition})") } else { condition.to_string() };
        let replacement = if elsif {
            let width = helpers::alignment::configured_indentation_width(cx) as usize;
            format!("else\n{}{expression}", " ".repeat(src.column(col_off) + width))
        } else {
            expression
        };
        let msg_body = if elsif { format!("\n{replacement}") } else { replacement.clone() };
        let msg = format!("This conditional expression can just be replaced by `{msg_body}`.");
        let (start, end) = (node.location().start_offset(), helpers::if_node::parser_end(node));
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
