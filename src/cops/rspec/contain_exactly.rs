use crate::prelude::*;

pub const NAME: &str = "RSpec/ContainExactly";
pub static COP: ContainExactly = ContainExactly;
pub struct ContainExactly;

impl Cop for ContainExactly {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "contain_exactly" {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        let Some(splat) = arg.as_splat_node() else { return };
        let Some(array) = splat.expression() else { return };
        let text = format!("match_array({})", node_text(&array));
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Prefer `match_array` when matching array values.", |c| c.replace(s, e, &text));
    }
}
