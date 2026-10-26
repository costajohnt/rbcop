use crate::prelude::*;
use crate::project_index::{self, Visibility};

pub const NAME: &str = "Lint/ArgumentMismatch";
pub static COP: ArgumentMismatch = ArgumentMismatch;
pub struct ArgumentMismatch;

/// Only with `AllCops/UseProjectIndex`: a call on a constant whose public singleton method has a
/// signature the arguments do not fit.
impl Cop for ArgumentMismatch {
    fn kinds(&self) -> &'static [usize] {
        project_index::SEND_KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(index) = project_index::get() else { return };
        let Some(send) = project_index::parser_send(node) else { return };
        let Some(recv) = send.receiver else { return };
        if (recv.as_constant_read_node().is_none() && recv.as_constant_path_node().is_none()) || send.name == "new" {
            return;
        }
        let Some(sel) = send.selector else { return };
        let Some(decl) = project_index::resolve_const_node(index, &recv, cx.ancestors) else { return };
        let msg = {
            let ix = index.read();
            if !ix.is_namespace(decl) || !ix.fully_resolved_ancestry(decl, false) {
                return;
            }
            let Some(member) = ix.singleton_member(decl, &format!("{}()", send.name)) else { return };
            if ix.visibility(member) != Some(Visibility::Public) {
                return;
            }
            let Some((min, max, keywords)) = ix.member_shape(member) else { return };
            let Some(given) = project_index::positional_argument_count(&send.args, keywords) else { return };
            if project_index::arity_satisfied(given, min, max) {
                return;
            }
            format!(
                "Wrong number of arguments to `{}` (given {given}, expected {}).",
                send.name,
                project_index::expected_range(min, max)
            )
        };
        cx.add(sel.0, sel.1, msg);
    }
}
