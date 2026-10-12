use crate::prelude::*;

pub const NAME: &str = "RSpec/IdenticalEqualityAssertion";
pub static COP: IdenticalEqualityAssertion = IdenticalEqualityAssertion;
pub struct IdenticalEqualityAssertion;

impl Cop for IdenticalEqualityAssertion {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send nil? :expect $_) :to {(send nil? {:eql :eq :be} $_) (send (send nil? :be) :== $_)})
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "to" {
            return;
        }
        let Some(expect) = call.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect")) else { return };
        let [left] = &send_args(&expect)[..] else { return };
        let [matcher] = &send_args(&call)[..] else { return };
        let Some(m) = helpers::rspec::send_call(matcher).filter(|m| !is_csend(m)) else { return };
        let [right] = &send_args(&m)[..] else { return };
        let ok = match m.receiver() {
            None => matches!(method_name(&m), "eql" | "eq" | "be"),
            Some(r) => method_name(&m) == "==" && helpers::rspec::bare_send(&r, "be").is_some(),
        };
        if ok && helpers::node_equality::same(cx.src, helpers::rspec::node_range(left), helpers::rspec::node_range(right)) {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Identical expressions on both sides of the equality may indicate a flawed test.");
        }
    }
}
