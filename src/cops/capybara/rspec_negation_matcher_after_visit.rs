use crate::prelude::*;
use helpers::capybara::{negative_matcher, positive_matcher};

pub const NAME: &str = "Capybara/RSpec/NegationMatcherAfterVisit";
pub static COP: NegationMatcherAfterVisit = NegationMatcherAfterVisit;
pub struct NegationMatcherAfterVisit;

/// `(send nil? name ...)` without a block, its name passing `pred`.
fn bare_call(node: &Node, pred: impl Fn(&str) -> bool) -> bool {
    node.as_call_node()
        .is_some_and(|c| !is_csend(&c) && c.receiver().is_none() && pred(method_name(&c)) && helpers::rspec::block_of(&c).is_none())
}

impl Cop for NegationMatcherAfterVisit {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "visit" || helpers::rspec::block_of(&call).is_some() {
            return;
        }
        let Some(sibling) = helpers::rspec::right_sibling(cx.ancestors, node) else { return };
        // (send (send nil? :expect _) {:to NEGATIVE | {:not_to :to_not} POSITIVE})
        let Some(runner) = sibling.as_call_node().filter(|c| !is_csend(c) && helpers::rspec::block_of(c).is_none()) else { return };
        let Some(expect) = runner.receiver() else { return };
        if !bare_call(&expect, |n| n == "expect") || send_args(&expect.as_call_node().unwrap()).len() != 1 {
            return;
        }
        let [m] = &send_args(&runner)[..] else { return };
        let ok = match method_name(&runner) {
            "to" => bare_call(m, negative_matcher),
            "not_to" | "to_not" => bare_call(m, positive_matcher),
            _ => false,
        };
        if ok {
            let (s, e) = send_range(&runner);
            cx.add(s, e, "Do not use negation matcher immediately after visit.");
        }
    }
}
