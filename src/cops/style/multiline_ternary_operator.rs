use crate::helpers::parser_parent::{is_ternary, parser_parent};
use crate::prelude::*;
use std::cell::Cell;

pub const NAME: &str = "Style/MultilineTernaryOperator";
pub static COP: MultilineTernaryOperator = MultilineTernaryOperator;
pub struct MultilineTernaryOperator;

const MSG_IF: &str = "Avoid multi-line ternary operators, use `if` or `unless` instead.";
const MSG_SINGLE_LINE: &str = "Avoid multi-line ternary operators, use single-line instead.";

/// The single statement of a ternary branch.
fn branch_text<'pr>(stmts: Option<ruby_prism::StatementsNode<'pr>>) -> &'pr str {
    stmts.and_then(|s| s.body().iter().next()).map_or("", |n| node_text(&n))
}

impl Cop for MultilineTernaryOperator {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !is_ternary(node) {
            return;
        }
        let n = node.as_if_node().unwrap();
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let src = cx.src;
        if src.line_of(start) == src.line_of(end) {
            return;
        }
        let parent = parser_parent(cx.ancestors);
        // SINGLE_LINE_TYPES = return break next send csend super yield, minus setter sends;
        // `defined?` without parentheses (1.92).
        let single_line = parent.is_some_and(|p| match kind::of(p) {
            kind::RETURN_NODE | kind::BREAK_NODE | kind::NEXT_NODE | kind::SUPER_NODE | kind::YIELD_NODE => true,
            kind::DEFINED_NODE => p.as_defined_node().unwrap().lparen_loc().is_none(),
            kind::CALL_NODE => !is_assignment_method(p),
            _ => false,
        });
        let cond = node_text(&n.predicate());
        let if_src = branch_text(n.statements());
        let else_src = branch_text(n.subsequent().and_then(|e| e.as_else_node()).and_then(|e| e.statements()));
        let replacement = if single_line {
            format!("{cond} ? {if_src} : {else_src}")
        } else {
            format!("if {cond}\n  {if_src}\nelse\n  {else_src}\nend")
        };
        if src.text[start..end] == replacement {
            return;
        }
        let msg = if single_line { MSG_SINGLE_LINE } else { MSG_IF };
        let comments: String = src
            .comments
            .iter()
            .filter(|c| c.start >= start && c.end <= end)
            .map(|c| format!("{}\n", &src.text[c.start..c.end]))
            .collect();
        let parent_range = parent.map(|_| {
            let r = parser_ancestor_ranges(cx.ancestors)[0];
            (r.1, r.2)
        });
        let ignored = cx.state::<Vec<(usize, usize)>>().iter().any(|&(a, b)| a <= start && end <= b);
        let ran = Cell::new(false);
        cx.add_fix(start, end, msg, |c| {
            if ignored {
                return;
            }
            c.replace(start, end, &replacement);
            if let Some((ps, pe)) = parent_range
                && !comments.is_empty()
            {
                c.insert_before(ps, pe, &comments);
            }
            ran.set(true);
        });
        if ran.get() {
            cx.state::<Vec<(usize, usize)>>().push((start, end));
        }
    }
}

/// `send_type? && assignment_method?` on a call whose name ends in `=` but is not a comparison.
fn is_assignment_method(node: &Node) -> bool {
    let call = node.as_call_node().unwrap();
    let name = method_name(&call);
    name.ends_with('=') && !matches!(name, "==" | "===" | "!=" | "<=" | ">=") && !is_csend(&call)
}
