use crate::helpers::configurable_naming::{forbidden_name, naming_valid, report_opposing_styles};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/StructPropName";
pub static COP: StructPropName = StructPropName;
pub struct StructPropName;

/// `within_t_struct?`: the nearest class/module/sclass/def is a class whose superclass is
/// `(const (const {nil? cbase} :T) {:Struct :ImmutableStruct :InexactStruct})`.
fn within_t_struct(ancestors: &[Node]) -> bool {
    let scope = ancestors.iter().rev().find(|n| {
        matches!(n, Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. } | Node::DefNode { .. })
    });
    scope.and_then(|s| s.as_class_node()).and_then(|c| c.superclass()).and_then(|s| s.as_constant_path_node()).is_some_and(|p| {
        p.name().is_some_and(|n| matches!(n.as_slice(), b"Struct" | b"ImmutableStruct" | b"InexactStruct"))
            && p.parent().is_some_and(|t| helpers::parser_parent::is_top_const(&t, "T"))
    })
}

impl Cop for StructPropName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "const" | "prop") || is_csend(&call) {
            return;
        }
        if !call.receiver().is_none_or(|r| r.as_self_node().is_some()) {
            return;
        }
        let Some(name_node) = send_args(&call).into_iter().next() else { return };
        let Some(sym) = name_node.as_symbol_node() else { return };
        if !within_t_struct(cx.ancestors) {
            return;
        }
        let name = String::from_utf8_lossy(sym.unescaped()).into_owned();
        if helpers::allowed::matches_allowed_pattern(cx.cfg, &name) {
            return;
        }
        let (s, e) = helpers::node_types::range(&name_node);
        if forbidden_name(cx.cfg, &name) {
            return cx.add(s, e, format!("`{name}` is forbidden, use another property name instead."));
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("snake_case").to_string();
        if naming_valid(&style, &name) {
            cx.correct_style_detected();
        } else {
            report_opposing_styles(cx, s, e, |st| naming_valid(st, &name));
            cx.add(s, e, format!("Use {style} for T::Struct property names."));
        }
    }
}
