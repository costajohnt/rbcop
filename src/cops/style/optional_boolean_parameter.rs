use crate::prelude::*;

pub const NAME: &str = "Style/OptionalBooleanParameter";
pub static COP: OptionalBooleanParameter = OptionalBooleanParameter;
pub struct OptionalBooleanParameter;

impl Cop for OptionalBooleanParameter {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
        if helpers::allowed::allowed_method(cx.cfg, name) {
            return;
        }
        let Some(params) = def.parameters() else { return };
        for opt in params.optionals().iter() {
            let opt = opt.as_optional_parameter_node().unwrap();
            let value = opt.value();
            if value.as_true_node().is_none() && value.as_false_node().is_none() {
                continue;
            }
            let loc = opt.location();
            let arg_name = std::str::from_utf8(opt.name().as_slice()).unwrap_or("");
            let msg = format!(
                "Prefer keyword arguments for arguments with a boolean default value; use `{arg_name}: {}` instead of `{}`.",
                node_text(&value),
                text(&loc)
            );
            cx.add(loc.start_offset(), loc.end_offset(), &msg);
        }
    }
}
