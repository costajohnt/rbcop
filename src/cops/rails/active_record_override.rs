use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/ActiveRecordOverride";
pub static COP: ActiveRecordOverride = ActiveRecordOverride;
pub struct ActiveRecordOverride;

impl Cop for ActiveRecordOverride {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.receiver().is_some() {
            return;
        }
        let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        if !matches!(name.as_str(), "create" | "destroy" | "save" | "update") {
            return;
        }
        // find_parent_class_name: the nearest enclosing class's superclass source.
        let Some(class) = cx.ancestors.iter().rev().find_map(|a| a.as_class_node()) else { return };
        let Some(sup) = class.superclass() else { return };
        if !matches!(node_text(&sup), "ApplicationRecord" | "ActiveModel::Base" | "ActiveRecord::Base") {
            return;
        }
        struct Zsuper(bool);
        impl<'pr> Visit<'pr> for Zsuper {
            fn visit_forwarding_super_node(&mut self, n: &ruby_prism::ForwardingSuperNode<'pr>) {
                self.0 = true;
                ruby_prism::visit_forwarding_super_node(self, n);
            }
        }
        let mut z = Zsuper(false);
        if let Some(b) = def.body() {
            z.visit(&b);
        }
        if let Some(p) = def.parameters() {
            z.visit(&p.as_node());
        }
        if !z.0 {
            return;
        }
        let msg = format!(
            "Use `before_{name}`, `around_{name}`, or `after_{name}` callbacks instead of overriding the Active Record method `{name}`."
        );
        cx.add(node.location().start_offset(), node.location().end_offset(), msg);
    }
}
