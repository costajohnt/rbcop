use crate::prelude::*;

pub const NAME: &str = "FactoryBot/FactoryNameStyle";
pub static COP: FactoryNameStyle = FactoryNameStyle;
pub struct FactoryNameStyle;

impl Cop for FactoryNameStyle {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send #factory_call? %FACTORY_CALLS ${str sym} ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !helpers::factory_bot::METHODS.contains(&method_name(&call)) {
            return;
        }
        if !helpers::factory_bot::factory_call(cx, call.receiver().as_ref()) {
            return;
        }
        let Some(name) = send_args(&call).into_iter().next() else { return };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("symbol").to_string();
        let prefer = if helpers::rspec::is_str(&name) && style == "symbol" {
            let v = name.as_string_node().unwrap().unescaped().to_vec();
            if v.contains(&b'/') {
                return;
            }
            helpers::ruby_string::symbol_inspect(&v)
        } else if helpers::rspec::is_sym(cx, &name) && style == "string" {
            helpers::ruby_string::inspect(name.as_symbol_node().unwrap().unescaped())
        } else {
            return;
        };
        let (s, e) = helpers::rspec::node_range(&name);
        cx.add_fix(s, e, format!("Use {style} to refer to a factory."), |c| c.replace(s, e, &prefer));
    }
}
