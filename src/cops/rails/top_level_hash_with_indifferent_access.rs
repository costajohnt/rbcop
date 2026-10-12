use crate::prelude::*;

pub const NAME: &str = "Rails/TopLevelHashWithIndifferentAccess";
pub static COP: TopLevelHashWithIndifferentAccess = TopLevelHashWithIndifferentAccess;
pub struct TopLevelHashWithIndifferentAccess;

impl Cop for TopLevelHashWithIndifferentAccess {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.1
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (const {nil? cbase} :HashWithIndifferentAccess)
        if !helpers::parser_parent::is_top_const(node, "HashWithIndifferentAccess") {
            return;
        }
        // node.parent&.class_type? && node.parent.ancestors.any?(&:module_type?)
        let anc = cx.ancestors;
        if anc.last().is_some_and(|p| p.as_class_node().is_some())
            && anc[..anc.len() - 1].iter().any(|a| a.as_module_node().is_some())
        {
            return;
        }
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        // insert_before(node.location.name): after a leading `::`.
        let (ns, ne) = node.as_constant_path_node().map_or((s, e), |p| (p.name_loc().start_offset(), p.name_loc().end_offset()));
        cx.add_fix(s, e, "Avoid top-level `HashWithIndifferentAccess`.", |c| c.insert_before(ns, ne, "ActiveSupport::"));
    }
}
