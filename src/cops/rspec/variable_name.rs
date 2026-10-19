use crate::prelude::*;

pub const NAME: &str = "RSpec/VariableName";
pub static COP: VariableName = VariableName;
pub struct VariableName;

impl Cop for VariableName {
    fn kinds(&self) -> &'static [usize] {
        helpers::rspec::SEND_KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::on_send(cx.ancestors, node) || !helpers::rspec::inside_example_group(&l, cx.ancestors, node) {
            return;
        }
        // an op-assign's or a target's send has a receiver: never a let or subject
        if node.as_call_node().is_none() {
            return;
        }
        let Some(variable) = helpers::rspec::variable_definition(cx, &l, node) else { return };
        let name = if helpers::rspec::is_str(&variable) {
            String::from_utf8_lossy(variable.as_string_node().unwrap().unescaped()).into_owned()
        } else if helpers::rspec::is_sym(cx, &variable) {
            helpers::rails::sym_value(&variable).unwrap_or_default()
        } else {
            return;
        };
        if helpers::allowed::matches_allowed_pattern(cx.cfg, &name) {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("snake_case").to_string();
        if helpers::configurable_naming::naming_valid(&style, &name) {
            return;
        }
        let (s, e) = helpers::rspec::node_range(&variable);
        cx.add(s, e, format!("Use {style} for variable names."));
    }
}
