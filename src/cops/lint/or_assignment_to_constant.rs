use crate::prelude::*;

pub const NAME: &str = "Lint/OrAssignmentToConstant";
pub static COP: OrAssignmentToConstant = OrAssignmentToConstant;
pub struct OrAssignmentToConstant;

impl Cop for OrAssignmentToConstant {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_OR_WRITE_NODE, kind::CONSTANT_PATH_OR_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let op = match (node.as_constant_or_write_node(), node.as_constant_path_or_write_node()) {
            (Some(n), _) => n.operator_loc(),
            (_, Some(n)) => n.operator_loc(),
            _ => return,
        };
        let in_def = cx.ancestors.iter().any(|a| a.as_def_node().is_some());
        let (s, e) = (op.start_offset(), op.end_offset());
        cx.add_fix(s, e, "Avoid using or-assignment with constants.", |c| {
            if !in_def {
                c.replace(s, e, "=");
            }
        });
    }
}
