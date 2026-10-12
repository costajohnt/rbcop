use crate::prelude::*;

pub const NAME: &str = "Rails/ActionFilter";
pub static COP: ActionFilter = ActionFilter;
pub struct ActionFilter;

const FILTER: &[&str] = &[
    "after_filter", "append_after_filter", "append_around_filter", "append_before_filter", "around_filter",
    "before_filter", "prepend_after_filter", "prepend_around_filter", "prepend_before_filter", "skip_after_filter",
    "skip_around_filter", "skip_before_filter", "skip_filter",
];
const ACTION: &[&str] = &[
    "after_action", "append_after_action", "append_around_action", "append_before_action", "around_action",
    "before_action", "prepend_after_action", "prepend_around_action", "prepend_before_action", "skip_after_action",
    "skip_around_action", "skip_before_action", "skip_action_callback",
];

impl Cop for ActionFilter {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // on_block (a plain block, any receiver) or on_send (no receiver).
        let block = helpers::rails::block_call(node).is_some();
        if !block && (call.receiver().is_some() || is_csend(&call)) {
            return;
        }
        let (bad, good) = if cx.cfg.str("EnforcedStyle") == Some("filter") { (ACTION, FILTER) } else { (FILTER, ACTION) };
        let name = method_name(&call);
        let Some(i) = bad.iter().position(|m| *m == name) else { return };
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let replacement = bad.iter().position(|m| *m == text(&sel)).map(|j| good[j]);
        cx.add_fix(s, e, format!("Prefer `{}` over `{name}`.", good[i]), |c| {
            if let Some(r) = replacement {
                c.replace(s, e, r);
            }
        });
    }
}
