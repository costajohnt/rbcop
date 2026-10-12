use crate::prelude::*;

pub const NAME: &str = "Rails/AttributeDefaultBlockValue";
pub static COP: AttributeDefaultBlockValue = AttributeDefaultBlockValue;
pub struct AttributeDefaultBlockValue;

impl Cop for AttributeDefaultBlockValue {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :attribute _ ?_ (hash <$(pair (sym :default) $_) ...>))
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "attribute") {
            return;
        }
        let args = send_args(&call);
        if !(2..=3).contains(&args.len()) {
            return;
        }
        let hash = args.last().unwrap();
        if helpers::rails::hash_elements(hash).is_none() {
            return;
        }
        let Some(pair) = helpers::rails::hash_pair(hash, "default") else { return };
        let value = helpers::rails::pair_value(&pair);
        // frozen_literal_default?: (send {(array) (hash)} :freeze)
        if let Some(c) = helpers::rails::send_node(&value)
            && method_name(&c) == "freeze"
            && send_args(&c).is_empty()
            && c.receiver().is_some_and(|r| {
                r.as_array_node().is_some_and(|a| a.elements().iter().next().is_none())
                    || r.as_hash_node().is_some_and(|h| h.elements().iter().next().is_none())
            })
        {
            return;
        }
        // TYPE_OFFENDERS: send array hash
        if helpers::rails::send_node(&value).is_none() && value.as_array_node().is_none() && value.as_hash_node().is_none() {
            return;
        }
        let (s, e) = (value.location().start_offset(), value.location().end_offset());
        let replacement = format!("-> {{ {} }}", node_text(&value));
        cx.add_fix(s, e, "Pass method in a block to `:default` option.", |c| c.replace(s, e, &replacement));
    }
}
