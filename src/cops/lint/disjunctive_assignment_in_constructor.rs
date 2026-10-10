use crate::prelude::*;

pub const NAME: &str = "Lint/DisjunctiveAssignmentInConstructor";
pub static COP: DisjunctiveAssignmentInConstructor = DisjunctiveAssignmentInConstructor;
pub struct DisjunctiveAssignmentInConstructor;

const OR_ASGN: &[usize] = &[
    kind::LOCAL_VARIABLE_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_OR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
    kind::CONSTANT_OR_WRITE_NODE,
    kind::CONSTANT_PATH_OR_WRITE_NODE,
    kind::CALL_OR_WRITE_NODE,
    kind::INDEX_OR_WRITE_NODE,
];

impl Cop for DisjunctiveAssignmentInConstructor {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.receiver().is_some() || def.name().as_slice() != b"initialize" {
            return;
        }
        // A body with rescue/ensure is a single `rescue`/`ensure` node in parser: no or_asgn.
        let Some(stmts) = def.body().and_then(|b| b.as_statements_node()) else { return };
        for line in stmts.body().iter() {
            if !OR_ASGN.contains(&kind::of(&line)) {
                break;
            }
            if let Some(w) = line.as_instance_variable_or_write_node() {
                let op = w.operator_loc();
                cx.add_fix(op.start_offset(), op.end_offset(), "Unnecessary disjunctive assignment. Use plain assignment.", |c| {
                    c.replace(op.start_offset(), op.end_offset(), "=")
                });
            }
        }
    }
}
