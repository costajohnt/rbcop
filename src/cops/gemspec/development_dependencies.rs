use crate::prelude::*;

pub const NAME: &str = "Gemspec/DevelopmentDependencies";
pub static COP: DevelopmentDependencies = DevelopmentDependencies;
pub struct DevelopmentDependencies;

impl Cop for DevelopmentDependencies {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("Gemfile");
        let args = send_args(&call);
        let shape = match (style, method_name(&call)) {
            // (send _ :add_development_dependency (str #forbidden_gem? ...) _? _?)
            ("Gemfile" | "gems.rb", "add_development_dependency") => (1..=3).contains(&args.len()),
            // (send _ :gem (str #forbidden_gem? ...))
            ("gemspec", "gem") => args.len() == 1,
            _ => false,
        };
        if !shape {
            return;
        }
        let Some(name) = args[0].as_string_node().filter(|_| !is_parser_dstr(&args[0])) else { return };
        if cx.cfg.strings("AllowedGems").iter().any(|g| g.as_bytes() == name.unescaped()) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, format!("Specify development dependencies in {style}."));
    }
}
