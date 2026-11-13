use crate::prelude::*;

pub const NAME: &str = "ThreadSafety/ClassAndModuleAttributes";
pub static COP: ClassAndModuleAttributes = ClassAndModuleAttributes;
pub struct ClassAndModuleAttributes;

impl Cop for ClassAndModuleAttributes {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `(send nil? ...)`: no receiver (so never a csend).
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() {
            return;
        }
        let offense = match method_name(&call) {
            "mattr_writer" | "mattr_accessor" | "cattr_writer" | "cattr_accessor" => true,
            "class_attribute" => !cx.cfg.params.truthy("ActiveSupportClassAttributeAllowed"),
            "attr" | "attr_accessor" | "attr_writer" | "attr_internal" | "attr_internal_accessor" | "attr_internal_writer" => {
                // defined_in_singleton_class?: the nearest def or sclass ancestor is an sclass.
                cx.ancestors.iter().rev().find_map(|a| match a {
                    Node::DefNode { .. } if a.as_def_node().unwrap().receiver().is_none() => Some(false),
                    Node::SingletonClassNode { .. } => Some(true),
                    _ => None,
                }) == Some(true)
            }
            _ => false,
        };
        if offense {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Avoid mutating class and module attributes.");
        }
    }
}
