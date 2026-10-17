use crate::prelude::*;

pub const NAME: &str = "RSpec/RemoveConst";
pub static COP: RemoveConst = RemoveConst;
pub struct RemoveConst;

impl Cop for RemoveConst {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ {:send | :__send__} (sym :remove_const) _)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !matches!(method_name(&call), "send" | "__send__") {
            return;
        }
        let [sym, _] = &send_args(&call)[..] else { return };
        if helpers::rspec::is_sym(cx, sym) && helpers::rails::sym_value(sym).as_deref() == Some("remove_const") {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Do not use remove_const in specs. Consider using e.g. `stub_const`.");
        }
    }
}
