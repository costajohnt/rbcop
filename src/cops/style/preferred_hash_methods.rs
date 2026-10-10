use crate::prelude::*;

pub const NAME: &str = "Style/PreferredHashMethods";
pub static COP: PreferredHashMethods = PreferredHashMethods;
pub struct PreferredHashMethods;

impl Cop for PreferredHashMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let verbose = cx.cfg.str("EnforcedStyle") == Some("verbose");
        let name = method_name(&call);
        let offending: &[&str] = if verbose { &["key?", "value?"] } else { &["has_key?", "has_value?"] };
        if !offending.contains(&name) || send_args(&call).len() != 1 {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let proper = |m: &str| if verbose { format!("has_{m}") } else { m.trim_start_matches("has_").to_string() };
        let msg = format!("Use `Hash#{}` instead of `Hash#{name}`.", proper(name));
        let fix = proper(text(&sel));
        cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| c.replace(sel.start_offset(), sel.end_offset(), &fix));
    }
}
