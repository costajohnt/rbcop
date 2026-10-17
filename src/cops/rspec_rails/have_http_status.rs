use crate::prelude::*;

pub const NAME: &str = "RSpecRails/HaveHttpStatus";
pub static COP: HaveHttpStatus = HaveHttpStatus;
pub struct HaveHttpStatus;

impl Cop for HaveHttpStatus {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send nil? :expect $(send $(send nil? #response_methods?) {:status :code})) $RUNNERS
        //   $(send nil? {:be :eq :eql :equal} ({int str} $_)))
        let call = node.as_call_node().unwrap();
        let to = method_name(&call);
        if is_csend(&call) || !helpers::rspec::is_runner(to) {
            return;
        }
        let Some(expect) = call.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect")) else { return };
        let [status_call] = &send_args(&expect)[..] else { return };
        let Some(status_call) = helpers::rspec::send_call(status_call).filter(|c| !is_csend(c)) else { return };
        if !matches!(method_name(&status_call), "status" | "code") || !send_args(&status_call).is_empty() {
            return;
        }
        let Some(response) = status_call.receiver().and_then(|r| helpers::rspec::send_call(&r)) else { return };
        let methods = cx.cfg.strings("ResponseMethods");
        if response.receiver().is_some() || !send_args(&response).is_empty() || !methods.iter().any(|m| m == method_name(&response)) {
            return;
        }
        let [matcher] = &send_args(&call)[..] else { return };
        let Some(matcher) = helpers::rspec::send_call(matcher) else { return };
        if matcher.receiver().is_some() || !matches!(method_name(&matcher), "be" | "eq" | "eql" | "equal") {
            return;
        }
        let [arg] = &send_args(&matcher)[..] else { return };
        let status = if arg.as_integer_node().is_some() {
            match int_literal_value(arg) {
                Some(v) => v.to_string(),
                None => return,
            }
        } else if helpers::rspec::is_str(arg) {
            String::from_utf8_lossy(arg.as_string_node().unwrap().unescaped()).into_owned()
        } else {
            return;
        };
        if status.is_empty() || !status.bytes().all(|b| b.is_ascii_digit()) {
            return;
        }
        let rname = method_name(&response).to_string();
        let (s, e) = send_range(&call);
        let msg = format!(
            "Prefer `expect({rname}).{to} have_http_status({status})` over `{}`.",
            cx.src.slice(s, e)
        );
        let (rs, re) = helpers::rspec::node_range(&status_call.as_node());
        let sel = matcher.message_loc().unwrap();
        let (ms, me) = (sel.start_offset(), sel.end_offset());
        let (as_, ae) = helpers::rspec::node_range(arg);
        cx.add_fix(s, e, msg, |c| {
            c.replace(rs, re, &rname);
            c.replace(ms, me, "have_http_status");
            c.replace(as_, ae, &status);
        });
    }
}
