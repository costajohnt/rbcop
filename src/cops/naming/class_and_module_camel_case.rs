use crate::prelude::*;

pub const NAME: &str = "Naming/ClassAndModuleCamelCase";
pub static COP: ClassAndModuleCamelCase = ClassAndModuleCamelCase;
pub struct ClassAndModuleCamelCase;

impl Cop for ClassAndModuleCamelCase {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let path = match node.as_class_node() {
            Some(c) => c.constant_path(),
            None => node.as_module_node().unwrap().constant_path(),
        };
        let loc = path.location();
        let name = node_text(&path);
        if !name.contains('_') {
            return;
        }
        // /#{AllowedNames.join('|')}/: a String has no join (a cop error).
        let allowed = cx.cfg.array("AllowedNames").join("|");
        let stripped = match crate::config::value::ruby_regex(&allowed) {
            Some(re) => re.replace_all(name, "").into_owned(),
            None => name.to_string(),
        };
        if stripped.contains('_') {
            cx.add(loc.start_offset(), loc.end_offset(), "Use CamelCase for classes and modules.");
        }
    }
}
