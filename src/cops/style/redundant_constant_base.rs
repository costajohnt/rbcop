use crate::prelude::*;

pub const NAME: &str = "Style/RedundantConstantBase";
pub static COP: RedundantConstantBase = RedundantConstantBase;
pub struct RedundantConstantBase;

// ponytail: the AllCops/UseProjectIndex (rubydex) branch is not ported; without the gem RuboCop
// never takes it.
impl Cop for RedundantConstantBase {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_PATH_NODE, kind::CONSTANT_PATH_TARGET_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `::Foo`: the cbase is the `::` of a path without a parent.
        let delim = match node {
            Node::ConstantPathNode { .. } => {
                let p = node.as_constant_path_node().unwrap();
                if p.parent().is_some() {
                    return;
                }
                p.delimiter_loc()
            }
            _ => {
                let p = node.as_constant_path_target_node().unwrap();
                if p.parent().is_some() {
                    return;
                }
                p.delimiter_loc()
            }
        };
        if cx.config.for_cop("Lint/ConstantResolution").truthy("Enabled") {
            return;
        }
        let off = delim.start_offset();
        // module_nesting_ancestors_of(node).none?: class/module ancestors, minus classes whose
        // superclass contains the node.
        let nested = cx.ancestors.iter().any(|a| match a {
            Node::ModuleNode { .. } => true,
            Node::ClassNode { .. } => !a.as_class_node().unwrap().superclass().is_some_and(|s| {
                let l = s.location();
                l.start_offset() <= off && off < l.end_offset()
            }),
            _ => false,
        });
        if nested {
            return;
        }
        let (start, end) = (off, delim.end_offset());
        cx.add_fix(start, end, "Remove redundant `::`.", |c| c.remove(start, end));
    }
}
