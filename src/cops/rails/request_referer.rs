use crate::prelude::*;

pub const NAME: &str = "Rails/RequestReferer";
pub static COP: RequestReferer = RequestReferer;
pub struct RequestReferer;

impl Cop for RequestReferer {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send nil? :request) {:referer :referrer})
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        if !matches!(name, "referer" | "referrer") || !send_args(&call).is_empty() {
            return;
        }
        let Some(recv) = call.receiver().and_then(|r| helpers::rails::send_node(&r)) else { return };
        if !helpers::rails::is_command(&recv, "request") || !send_args(&recv).is_empty() {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("referer").to_string();
        let wrong = if style == "referer" { "referrer" } else { "referer" };
        if name != wrong {
            return;
        }
        let (s, e) = send_range(&call);
        let replacement = format!("request.{style}");
        cx.add_fix(s, e, format!("Use `request.{style}` instead of `request.{wrong}`."), |c| c.replace(s, e, &replacement));
    }
}
