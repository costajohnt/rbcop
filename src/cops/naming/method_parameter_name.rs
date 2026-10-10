use crate::prelude::*;

pub const NAME: &str = "Naming/MethodParameterName";
pub static COP: MethodParameterName = MethodParameterName;
pub struct MethodParameterName;

impl Cop for MethodParameterName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(params) = node.as_def_node().unwrap().parameters() else { return };
        let args = parameter_nodes(&params);
        helpers::uncommunicative_name::check(cx, &args, "method parameter");
    }
}
