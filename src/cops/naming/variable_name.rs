use crate::helpers::configurable_naming::{self as naming, VarKind};
use crate::prelude::*;

pub const NAME: &str = "Naming/VariableName";
pub static COP: VariableName = VariableName;
pub struct VariableName;

impl Cop for VariableName {
    fn kinds(&self) -> &'static [usize] {
        naming::VARIABLE_KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // Below 2.7 a `_1` read is parser's `send`, which `on_lvar` never sees (#362).
        if helpers::node_types::is_legacy_numbered_read(cx, node) {
            return;
        }
        let Some((k, name, start, end)) = naming::variable(node, cx.ancestors) else { return };
        if k == VarKind::Gvasgn {
            if naming::forbidden_name(cx.cfg, &name) {
                cx.add(start, end, format!("`{name}` is forbidden, use another name instead."));
            }
            return;
        }
        if naming::allowed_identifier(cx.cfg, &name) {
            return;
        }
        if naming::forbidden_name(cx.cfg, &name) {
            return cx.add(start, end, format!("`{name}` is forbidden, use another name instead."));
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("snake_case");
        if naming::naming_valid(style, &name) || helpers::allowed::matches_allowed_pattern(cx.cfg, &name) {
            return cx.correct_style_detected();
        }
        naming::report_opposing_styles(cx, start, end, |s| naming::naming_valid(s, &name));
        cx.add(start, end, format!("Use {style} for variable names."));
    }
}
