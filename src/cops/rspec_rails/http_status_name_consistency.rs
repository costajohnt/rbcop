use crate::prelude::*;

pub const NAME: &str = "RSpecRails/HttpStatusNameConsistency";
pub static COP: HttpStatusNameConsistency = HttpStatusNameConsistency;
pub struct HttpStatusNameConsistency;

impl Cop for HttpStatusNameConsistency {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // requires_gem 'rack', '>= 3.1.0'
        if !cx.config.gem_at_least("rack", "3.1.0") {
            return;
        }
        // (send nil? :have_http_status ${sym})
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "have_http_status" {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        if !helpers::rspec::is_sym(cx, arg) {
            return;
        }
        let current = helpers::rails::sym_value(arg).unwrap_or_default();
        let preferred = match current.as_str() {
            "unprocessable_entity" => "unprocessable_content",
            "payload_too_large" => "content_too_large",
            _ => return,
        };
        let (s, e) = helpers::rspec::node_range(arg);
        let replacement = format!(":{preferred}");
        cx.add_fix(s, e, format!("Prefer `:{preferred}` over `:{current}`."), |c| c.replace(s, e, &replacement));
    }
}
