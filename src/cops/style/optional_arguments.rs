use crate::prelude::*;

pub const NAME: &str = "Style/OptionalArguments";
pub static COP: OptionalArguments = OptionalArguments;
pub struct OptionalArguments;

impl Cop for OptionalArguments {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(params) = node.as_def_node().unwrap().parameters() else { return };
        let args = parameter_nodes(&params);
        let Some(last_arg) = args.iter().rposition(|a| kind::of(a) == kind::REQUIRED_PARAMETER_NODE) else { return };
        for a in &args[..last_arg] {
            if kind::of(a) == kind::OPTIONAL_PARAMETER_NODE {
                let loc = a.location();
                cx.add(loc.start_offset(), loc.end_offset(), "Optional arguments should appear at the end of the argument list.");
            }
        }
    }
}
