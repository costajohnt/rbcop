use crate::prelude::*;

pub const NAME: &str = "Lint/AmbiguousAssignment";
pub static COP: AmbiguousAssignment = AmbiguousAssignment;
pub struct AmbiguousAssignment;

impl Cop for AmbiguousAssignment {
    fn kinds(&self) -> &'static [usize] {
        helpers::check_assignment::KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `node.loc.operator`: assignments, and setter sends (`a.b = x`, `a[i] = x`).
        let operator_end = match node.as_call_node() {
            Some(call) => call.equal_loc().map(|l| l.end_offset()),
            None => assignment_parts(node).map(|(op, _)| op.end_offset()),
        };
        let Some(rhs) = helpers::check_assignment::extract_rhs(node, true) else { return };
        let Some(operator_end) = operator_end else { return };
        let (start, end) = (operator_end - 1, rhs.location().start_offset() + 1);
        let op = match cx.src.text.get(start..end) {
            Some("=-") => "-=",
            Some("=+") => "+=",
            Some("=*") => "*=",
            Some("=!") => "!=",
            _ => return,
        };
        cx.add(start, end, format!("Suspicious assignment detected. Did you mean `{op}`?"));
    }
}
