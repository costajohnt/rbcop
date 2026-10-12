use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/NoExpectationExample";
pub static COP: NoExpectationExample = NoExpectationExample;
pub struct NoExpectationExample;

impl Cop for NoExpectationExample {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (any_block (send nil? {#Examples.regular #Examples.focused} ...) ...)
        let Some(call) = helpers::rspec::any_block_call(node) else { return };
        let l = helpers::rspec::lang(cx);
        let n = method_name(&call);
        if call.receiver().is_some() || !(l.ex_regular.iter().any(|x| x == n) || l.ex_focused.iter().any(|x| x == n)) {
            return;
        }
        // includes_expectation? / includes_skip_example?: receiver-less sends in the block
        struct V<'a> {
            l: &'a helpers::rspec::Language,
            cfg: &'a crate::config::CopConfig,
            expectation: bool,
            skip: bool,
        }
        impl<'pr> Visit<'pr> for V<'_> {
            fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                if n.receiver().is_none() {
                    let name = method_name(n);
                    if self.l.expectation(name) || helpers::allowed::matches_allowed_pattern(self.cfg, name) {
                        self.expectation = true;
                    }
                    if matches!(name, "pending" | "skip") {
                        self.skip = true;
                    }
                }
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V { l: &l, cfg: cx.cfg, expectation: false, skip: false };
        v.visit(node);
        if v.expectation || v.skip || helpers::rspec::skipped_in_metadata(cx, &call) {
            return;
        }
        let (s, e) = helpers::rspec::node_range(node);
        cx.add(s, e, "No expectation found in this example.");
    }
}
