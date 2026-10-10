use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/TimeNow";
pub static COP: TimeNow = TimeNow;
pub struct TimeNow;

impl Cop for TimeNow {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // `(call (const {nil? cbase} :Time) :new)`
        if method_name(&call) != "new" || !send_args(&call).is_empty() {
            return;
        }
        if !call.receiver().is_some_and(|r| is_top_const(&r, "Time")) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = send_range(&call);
        let sel_start = sel.start_offset();
        cx.add_fix(s, e, "Prefer `Time.now` over `Time.new` to retrieve the current time.", |c| {
            c.replace(sel_start, e, "now")
        });
    }
}
