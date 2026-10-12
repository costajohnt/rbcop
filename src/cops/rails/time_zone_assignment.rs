use crate::prelude::*;

pub const NAME: &str = "Rails/TimeZoneAssignment";
pub static COP: TimeZoneAssignment = TimeZoneAssignment;
const MSG: &str = "Use `Time.use_zone` with block instead of `Time.zone=`.";

pub struct TimeZoneAssignment;

impl Cop for TimeZoneAssignment {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::CALL_TARGET_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? cbase} :Time) :zone= ...)
        // A multiple-assignment target `a, Time.zone = ...` is a parser send too.
        if let Some(t) = node.as_call_target_node() {
            if !t.is_safe_navigation() && t.name().as_slice() == b"zone=" && helpers::rails::is_const(&t.receiver(), "Time") {
                cx.add(node.location().start_offset(), node.location().end_offset(), MSG);
            }
            return;
        }
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "zone=" || !call.receiver().is_some_and(|r| helpers::rails::is_const(&r, "Time")) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, MSG);
    }
}
