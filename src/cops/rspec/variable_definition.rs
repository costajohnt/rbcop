use crate::prelude::*;

pub const NAME: &str = "RSpec/VariableDefinition";
pub static COP: VariableDefinition = VariableDefinition;
pub struct VariableDefinition;

impl Cop for VariableDefinition {
    fn correctable(&self) -> bool {
        true
    }

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
        let symbols = cx.cfg.str("EnforcedStyle") == Some("symbols");
        let strings = cx.cfg.str("EnforcedStyle") == Some("strings");
        let is_sym = variable.as_symbol_node().is_some() || variable.as_interpolated_symbol_node().is_some();
        let replacement = if symbols && helpers::rspec::is_str(&variable) {
            // value.to_sym.inspect
            helpers::ruby_string::symbol_inspect(variable.as_string_node().unwrap().unescaped())
        } else if strings && is_sym {
            if helpers::rspec::is_dsym(cx, &variable) {
                node_text(&variable)[1..].to_string()
            } else {
                helpers::ruby_string::inspect(variable.as_symbol_node().unwrap().unescaped())
            }
        } else {
            return;
        };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or_default().to_string();
        let (s, e) = helpers::rspec::node_range(&variable);
        cx.add_fix(s, e, format!("Use {style} for variable names."), |c| c.replace(s, e, &replacement));
    }
}
