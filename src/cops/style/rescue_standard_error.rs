use crate::prelude::*;
use helpers::parser_parent::is_top_const;

pub const NAME: &str = "Style/RescueStandardError";
pub static COP: RescueStandardError = RescueStandardError;
pub struct RescueStandardError;

impl Cop for RescueStandardError {
    fn correctable(&self) -> bool {
        true
    }

    // RescueModifierNode is `rescue_modifier?`, which the cop skips.
    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let r = node.as_rescue_node().unwrap();
        let kw = r.keyword_loc();
        let (ks, ke) = (kw.start_offset(), kw.end_offset());
        let exceptions: Vec<_> = r.exceptions().iter().collect();
        if cx.cfg.str("EnforcedStyle") == Some("explicit") {
            if exceptions.is_empty() {
                let msg = "Avoid rescuing without specifying an error class.";
                cx.add_fix(ks, ke, msg, |c| c.insert_after(ks, ke, " StandardError"));
            }
        } else if let [e] = exceptions.as_slice()
            && is_top_const(e, "StandardError")
        {
            let end = e.location().end_offset();
            let msg = "Omit the error class when rescuing `StandardError` by itself.";
            cx.add_fix(ks, end, msg, |c| c.remove(ke, end));
        }
    }
}
