use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "ThreadSafety/EnvMutation";
pub static COP: EnvMutation = EnvMutation;
pub struct EnvMutation;

const MSG: &str = "Avoid mutating `ENV` due to its process-wide effect.";

const METHODS: &[&str] = &[
    "[]=", "store", "update", "merge!", "replace", "delete", "delete_if", "reject!", "keep_if", "select!", "filter!", "shift",
    "clear",
];

impl Cop for EnvMutation {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::INDEX_TARGET_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `a, ENV["X"] = ...`: the parser target is `(send (const nil :ENV) :[]= (str "X"))`.
        if let Some(t) = node.as_index_target_node() {
            if is_top_const(&t.receiver(), "ENV") {
                let l = node.location();
                cx.add(l.start_offset(), l.end_offset(), MSG);
            }
            return;
        }
        // (call (const {nil? cbase} :ENV) {:[]= :store ...} ...)
        let call = node.as_call_node().unwrap();
        if !METHODS.contains(&method_name(&call)) || !call.receiver().is_some_and(|r| is_top_const(&r, "ENV")) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, MSG);
    }
}
