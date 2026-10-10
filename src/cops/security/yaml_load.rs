use crate::prelude::*;

pub const NAME: &str = "Security/YAMLLoad";
pub static COP: YAMLLoad = YAMLLoad;
pub struct YAMLLoad;

impl Cop for YAMLLoad {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (0.0, 3.0)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call)
            || method_name(&call) != "load"
            || call.receiver().is_none_or(|r| helpers::const_name::const_name(&r).as_deref() != Some("YAML"))
        {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Prefer using `YAML.safe_load` over `YAML.load`.", |c| c.replace(s, e, "safe_load"));
    }
}
