use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/DiscardedMatcher";
pub static COP: DiscardedMatcher = DiscardedMatcher;
pub struct DiscardedMatcher;

fn matcher_call(call: &CallNode, custom: &[String]) -> bool {
    let n = method_name(call);
    call.receiver().is_none()
        && (matches!(n, "change" | "have_received" | "output" | "receive" | "receive_messages" | "receive_message_chain")
            || custom.iter().any(|c| c == n))
}

/// example_with_matcher_expectation?: a `to`/`not_to`/`to_not` send in the example with a matcher
/// call in its arguments.
fn has_expectation(example: &Node, custom: &[String]) -> bool {
    struct Inner<'a>(&'a [String], bool);
    impl<'pr> Visit<'pr> for Inner<'_> {
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            self.1 |= !is_csend(n) && matcher_call(n, self.0);
            ruby_prism::visit_call_node(self, n);
        }
    }
    struct V<'a>(&'a [String], bool);
    impl<'pr> Visit<'pr> for V<'_> {
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            if !self.1 && !is_csend(n) && helpers::rspec::is_runner(method_name(n)) {
                for a in send_args(n) {
                    let mut i = Inner(self.0, false);
                    i.visit(&a);
                    self.1 |= i.1;
                }
            }
            ruby_prism::visit_call_node(self, n);
        }
    }
    let mut v = V(custom, false);
    // each_descendant: below the example block
    let c = example.as_call_node().unwrap();
    if let Some(r) = c.receiver() {
        v.visit(&r);
    }
    for a in send_args(&c) {
        v.visit(&a);
    }
    if let Some(b) = c.block() {
        v.visit(&b);
    }
    v.1
}

impl Cop for DiscardedMatcher {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        // on_send, or on_block for a plain block (numblocks have only their send checked, whose
        // parent is no `block`)
        if helpers::rspec::block_of(&call).is_some_and(|b| !helpers::rspec::is_plain(&b)) {
            return;
        }
        let custom = cx.cfg.strings("CustomMatcherMethods");
        if !matcher_call(&call, &custom) {
            return;
        }
        let l = helpers::rspec::lang(cx);
        let anc = cx.ancestors;
        // inside_example?: the nearest example block around
        let Some(example) = anc.iter().rev().find(|a| helpers::rspec::example(&l, a)) else { return };
        if !has_expectation(example, &custom) {
            return;
        }
        // find_outermost_chain: up while the node is its parent's receiver
        let mut cur = anc.len();
        let mut range = helpers::rspec::node_range(node);
        while cur > 0 {
            let Some(p) = anc[cur - 1].as_call_node() else { break };
            if p.receiver().is_none_or(|r| helpers::rspec::node_range(&r) != range) {
                break;
            }
            cur -= 1;
            range = helpers::rspec::node_range(&anc[cur]);
            // a send with a block: its parent is that block (no example), so no offense
            if helpers::rspec::block_of(&p).is_some() {
                return;
            }
        }
        // void_value?: the target's parent is an example block, or a begin/case/when in void context
        let mut i = cur;
        let void = loop {
            let Some(pi) = helpers::parser_parent::parser_parent_index(anc, i) else { break false };
            let p = &anc[pi];
            match kind::of(p) {
                kind::BLOCK_NODE => break pi > 0 && helpers::rspec::example(&l, &anc[pi - 1]),
                kind::STATEMENTS_NODE | kind::PARENTHESES_NODE | kind::CASE_NODE | kind::WHEN_NODE => i = pi,
                _ => break false,
            }
        };
        if !void {
            return;
        }
        let (s, e) = if cur == anc.len() { send_range(&call) } else { range };
        // a matcher with a block is reported with its block
        let (s, e) = if cur == anc.len() && helpers::rspec::block_of(&call).is_some() { helpers::rspec::node_range(node) } else { (s, e) };
        cx.add(s, e, format!("The result of `{}` is not used. Did you mean to chain it with `.and`?", method_name(&call)));
    }
}
