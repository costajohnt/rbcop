use crate::prelude::*;

pub const NAME: &str = "Naming/AccessorMethodName";
pub static COP: AccessorMethodName = AccessorMethodName;
pub struct AccessorMethodName;

// ponytail: overrides_inherited_method? needs AllCops/UseProjectIndex (default false), not ported.
impl Cop for AccessorMethodName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let name = def.name().as_slice();
        if name.ends_with(b"!") || name.ends_with(b"?") || name.ends_with(b"=") {
            return;
        }
        let args = def.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default();
        let msg = if name.starts_with(b"get_") && args.is_empty() {
            "Do not prefix reader method names with `get_`."
        } else if name.starts_with(b"set_") && args.len() == 1 && args[0].as_required_parameter_node().is_some() {
            "Do not prefix writer method names with `set_`."
        } else {
            return;
        };
        let loc = def.name_loc();
        cx.add(loc.start_offset(), loc.end_offset(), msg);
    }
}
