use crate::helpers::alignment as alignment;
use crate::helpers::multiline_expression_indentation::{Ctx, indentation};
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineOperationIndentation";
pub static COP: MultilineOperationIndentation = MultilineOperationIndentation;
pub struct MultilineOperationIndentation;

const OPERATORS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

impl Cop for MultilineOperationIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::AND_NODE, kind::OR_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (range, lhs, rhs) = match kind::of(node) {
            kind::AND_NODE => {
                let n = node.as_and_node().unwrap();
                (span(node), span(&n.left()), span(&n.right()))
            }
            kind::OR_NODE => {
                let n = node.as_or_node().unwrap();
                (span(node), span(&n.left()), span(&n.right()))
            }
            _ => {
                let call = node.as_call_node().unwrap();
                let Some(receiver) = call.receiver() else { return };
                let name = method_name(&call);
                if name == "[]" || call.call_operator_loc().is_some() {
                    return;
                }
                // unary_operation? (send only): operator whose selector starts the expression.
                let start = node.location().start_offset();
                if !is_csend(&call)
                    && call.message_loc().is_some_and(|m| m.start_offset() == start)
                    && OPERATORS.contains(&name)
                {
                    return;
                }
                let Some(first) = send_args(&call).into_iter().next() else { return };
                (send_range(&call), span(&receiver), span(&first))
            }
        };
        let src = cx.src;
        // Each way out without an offense is `correct_style_detected` (offending_range is falsy).
        if !alignment::begins_its_line(src, rhs.0) {
            return cx.correct_style_detected();
        }
        let ctx = Ctx::new(cx.ancestors, node, range);
        if ctx.not_for_this_cop() {
            return cx.correct_style_detected();
        }
        let aligned = cx.cfg.str("EnforcedStyle") != Some("indented");
        let width = alignment::configured_indentation_width(cx);
        // offending_range's column delta for a style
        let delta_for = |aligned: bool| {
            let correct = if should_align(cx, &ctx, rhs, aligned) {
                src.column(range.0) as i64
            } else {
                indentation(cx, lhs.0) + ctx.correct_indentation(cx, width)
            };
            correct - src.column(rhs.0) as i64
        };
        let align = should_align(cx, &ctx, rhs, aligned);
        let delta = delta_for(aligned);
        if delta == 0 {
            return cx.correct_style_detected();
        }
        // incorrect_style_detected: the other style would be wrong too, or is what is used.
        if cx.block_runs(rhs.0, rhs.1) {
            if delta_for(!aligned) != 0 {
                cx.no_acceptable_style();
            } else {
                cx.opposite_style_detected();
            }
        }
        let what = ctx.operation_description(Some(rhs));
        let msg = if align {
            format!("Align the operands of {what} spanning multiple lines.")
        } else {
            let used = src.column(rhs.0) as i64 - indentation(cx, lhs.0);
            format!(
                "Use {} (not {used}) spaces for indenting {what} spanning multiple lines.",
                ctx.correct_indentation(cx, width)
            )
        };
        let set = helpers::alignment_corrector::Settings::of(cx);
        cx.add_fix(rhs.0, rhs.1, msg, |c| helpers::alignment_corrector::correct(c, src, set, rhs.0, rhs.1, None, delta, false));
    }
}

fn should_align(cx: &Cx, ctx: &Ctx, rhs: (usize, usize), aligned: bool) -> bool {
    let assignment = ctx.part_of_assignment_rhs(Some(rhs));
    if let Some(a) = assignment
        && ctx.assignment_rhs(a).is_some_and(|r| alignment::begins_its_line(cx.src, r.0))
    {
        return true;
    }
    if !aligned {
        return false;
    }
    if ctx.keyword_node().is_some() || assignment.is_some() {
        return true;
    }
    ctx.argument_in_method_call(false).is_some_and(|a| !ctx.def_modifier(a))
}
