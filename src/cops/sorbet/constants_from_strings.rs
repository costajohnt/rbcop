use crate::prelude::*;

pub const NAME: &str = "Sorbet/ConstantsFromStrings";
pub static COP: ConstantsFromStrings = ConstantsFromStrings;
pub struct ConstantsFromStrings;

impl Cop for ConstantsFromStrings {
    fn kinds(&self) -> &'static [usize] {
        // The send inside `a.constants ||= x` (parser `op_asgn` / `or_asgn` / `and_asgn`) too.
        &[kind::CALL_NODE, kind::CALL_OPERATOR_WRITE_NODE, kind::CALL_OR_WRITE_NODE, kind::CALL_AND_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (name, sel) = match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                (call.name(), call.message_loc())
            }
            Node::CallOperatorWriteNode { .. } => {
                let n = node.as_call_operator_write_node().unwrap();
                (n.read_name(), n.message_loc())
            }
            Node::CallOrWriteNode { .. } => {
                let n = node.as_call_or_write_node().unwrap();
                (n.read_name(), n.message_loc())
            }
            _ => {
                let n = node.as_call_and_write_node().unwrap();
                (n.read_name(), n.message_loc())
            }
        };
        let name = std::str::from_utf8(name.as_slice()).unwrap_or("");
        if !matches!(name, "constantize" | "constants" | "const_get" | "safe_constantize") {
            return;
        }
        let Some(sel) = sel else { return };
        cx.add(
            sel.start_offset(),
            sel.end_offset(),
            format!(
                "Don't use `{name}`, it makes the code harder to understand, less editor-friendly, and impossible to analyze. Replace `{name}` with a case/when or a hash."
            ),
        );
    }
}
