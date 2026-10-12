use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/ReversibleMigrationMethodDefinition";
pub static COP: ReversibleMigrationMethodDefinition = ReversibleMigrationMethodDefinition;
pub struct ReversibleMigrationMethodDefinition;

impl Cop for ReversibleMigrationMethodDefinition {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !helpers::rails::is_migration_class(&class) {
            return;
        }
        // `(def :name (args) _) anywhere below: plain defs without parameters.
        struct Defs(Vec<Vec<u8>>);
        impl<'pr> Visit<'pr> for Defs {
            fn visit_def_node(&mut self, d: &ruby_prism::DefNode<'pr>) {
                let no_params = d.parameters().is_none_or(|p| parameter_nodes(&p).is_empty());
                if d.receiver().is_none() && no_params {
                    self.0.push(d.name().as_slice().to_vec());
                }
                ruby_prism::visit_def_node(self, d);
            }
        }
        let mut defs = Defs(Vec::new());
        defs.visit(node);
        let has = |n: &[u8]| defs.0.iter().any(|d| d == n);
        if has(b"change") || (has(b"up") && has(b"down")) {
            return;
        }
        cx.add(
            node.location().start_offset(),
            node.location().end_offset(),
            "Migrations must contain either a `change` method, or both an `up` and a `down` method.",
        );
    }
}
