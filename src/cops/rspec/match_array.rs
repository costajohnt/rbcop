use crate::prelude::*;

pub const NAME: &str = "RSpec/MatchArray";
pub static COP: MatchArray = MatchArray;
pub struct MatchArray;

impl Cop for MatchArray {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "match_array" {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first().and_then(|a| a.as_array_node()) else { return };
        // (send nil? :match_array (array))
        if call.receiver().is_none() && args.len() == 1 && first.elements().iter().next().is_none() {
            return;
        }
        // percent_literal?
        if first.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%")) {
            return;
        }
        // `node.arguments.flat_map(&:to_a)`: any other argument's children are not all nodes (RuboCop raises)
        let mut contents = Vec::new();
        for a in &args {
            let Some(arr) = a.as_array_node() else { return };
            contents.extend(arr.elements().iter().map(|e| node_text(&e).to_string()));
        }
        let replacement = format!("contain_exactly({})", contents.join(", "));
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Prefer `contain_exactly` when matching an array literal.", |c| c.replace(s, e, &replacement));
    }
}
