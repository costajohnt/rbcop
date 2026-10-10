use crate::prelude::*;

pub const NAME: &str = "Lint/UriRegexp";
pub static COP: UriRegexp = UriRegexp;
pub struct UriRegexp;

impl Cop for UriRegexp {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "regexp" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        // (const {cbase nil?} :URI)
        let uri = match (recv.as_constant_read_node(), recv.as_constant_path_node()) {
            (Some(c), _) => c.name().as_slice() == b"URI",
            (_, Some(p)) => p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"URI"),
            _ => false,
        };
        if !uri {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let parser = if cx.config.target_ruby >= 3.4 { "RFC2396_PARSER" } else { "DEFAULT_PARSER" };
        let argument = send_args(&call).first().map(|a| format!("({})", node_text(a))).unwrap_or_default();
        let preferred = format!("{}::{parser}.make_regexp{argument}", node_text(&recv));
        let (s, e) = send_range(&call);
        let msg = format!("`{}` is obsolete and should not be used. Instead, use `{preferred}`.", &cx.src.text[s..e]);
        cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| c.replace(s, e, &preferred));
    }
}
