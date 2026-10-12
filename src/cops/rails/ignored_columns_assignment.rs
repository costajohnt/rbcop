use crate::prelude::*;

pub const NAME: &str = "Rails/IgnoredColumnsAssignment";
pub static COP: IgnoredColumnsAssignment = IgnoredColumnsAssignment;
pub struct IgnoredColumnsAssignment;

impl Cop for IgnoredColumnsAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "ignored_columns=" {
            return;
        }
        let Some(op) = call.equal_loc() else { return };
        let (s, e) = (op.start_offset(), op.end_offset());
        cx.add_fix(s, e, "Use `+=` instead of `=`.", |c| c.replace(s, e, "+="));
    }
}
