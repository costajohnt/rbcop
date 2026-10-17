use crate::prelude::*;

pub const NAME: &str = "Rails/DefaultScope";
pub static COP: DefaultScope = DefaultScope;
pub struct DefaultScope;

const MSG: &str = "Avoid use of `default_scope`. It is better to use explicitly named scopes.";

impl Cop for DefaultScope {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::DEF_NODE, kind::SINGLETON_CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node {
            Node::CallNode { .. } => {
                let Some(call) = helpers::rails::send(node) else { return };
                if helpers::rails::is_command(&call, "default_scope") {
                    let sel = call.message_loc().unwrap();
                    cx.add(sel.start_offset(), sel.end_offset(), MSG);
                }
            }
            Node::DefNode { .. } => {
                // (defs _ :default_scope args ...)
                let def = node.as_def_node().unwrap();
                if def.receiver().is_some() && def.name().as_slice() == b"default_scope" {
                    let n = def.name_loc();
                    cx.add(n.start_offset(), n.end_offset(), MSG);
                }
            }
            _ => {
                // (sclass (self) $(def :default_scope args ...))
                let sc = node.as_singleton_class_node().unwrap();
                if sc.expression().as_self_node().is_none() {
                    return;
                }
                let stmts = helpers::rails::body_statements(sc.body());
                let [only] = stmts.as_slice() else { return };
                let Some(def) = only.as_def_node() else { return };
                if def.receiver().is_none() && def.name().as_slice() == b"default_scope" {
                    let n = def.name_loc();
                    cx.add(n.start_offset(), n.end_offset(), MSG);
                }
            }
        }
    }
}
