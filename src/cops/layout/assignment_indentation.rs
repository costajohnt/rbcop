use crate::helpers::alignment::{self, Item};
use crate::prelude::*;

pub const NAME: &str = "Layout/AssignmentIndentation";
pub static COP: AssignmentIndentation = AssignmentIndentation;
pub struct AssignmentIndentation;

static KINDS: LazyLock<Vec<usize>> = LazyLock::new(|| [ASSIGNMENT_KINDS, &[kind::CALL_NODE]].concat());

impl Cop for AssignmentIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // CheckAssignment: assignments, plus sends that have an operator (setters; on_send only).
        let (op, rhs) = match node.as_call_node() {
            Some(call) => {
                let (Some(eq), Some(last)) = (call.equal_loc(), call_args(&call).pop()) else { return };
                if is_csend(&call) {
                    return;
                }
                (eq, last)
            }
            None => match assignment_parts(node) {
                Some(p) => p,
                None => return,
            },
        };
        let src = cx.src;
        if src.line_of(op.start_offset()) == src.line_of(rhs.location().start_offset()) {
            return;
        }
        let start = node.location().start_offset();
        let leftmost = cx
            .ancestors
            .iter()
            .rev()
            .find(|p| kind::of(p) != kind::ARGUMENTS_NODE)
            .filter(|p| is_assignment(p) && src.line_of(p.location().start_offset()) == src.line_of(start))
            .map_or(start, |p| p.location().start_offset());
        let base = alignment::display_column(src, leftmost) as i64 + alignment::configured_indentation_width(cx);
        let msg = "Indent the first line of the right-hand-side of a multi-line assignment.";
        alignment::check_alignment(cx, &[Item::of(rhs)], Some(base), msg);
    }
}
