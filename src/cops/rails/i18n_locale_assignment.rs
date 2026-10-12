use crate::prelude::*;

pub const NAME: &str = "Rails/I18nLocaleAssignment";
pub static COP: I18nLocaleAssignment = I18nLocaleAssignment;
const MSG: &str = "Use `I18n.with_locale` with block instead of `I18n.locale=`.";

pub struct I18nLocaleAssignment;

impl Cop for I18nLocaleAssignment {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::CALL_TARGET_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? cbase} :I18n) :locale= ...)
        // A multiple-assignment target `a, I18n.locale = ...` is a parser send too.
        if let Some(t) = node.as_call_target_node() {
            if !t.is_safe_navigation() && t.name().as_slice() == b"locale=" && helpers::rails::is_const(&t.receiver(), "I18n") {
                cx.add(node.location().start_offset(), node.location().end_offset(), MSG);
            }
            return;
        }
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "locale=" || !call.receiver().is_some_and(|r| helpers::rails::is_const(&r, "I18n")) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, MSG);
    }
}
