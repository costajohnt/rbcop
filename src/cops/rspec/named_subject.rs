use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/NamedSubject";
pub static COP: NamedSubject = NamedSubject;
pub struct NamedSubject;

impl Cop for NamedSubject {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        // example_or_hook_block?: (block (send nil? {#Examples.all #Hooks.all} ...) ...)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if call.receiver().is_some() || !(l.example(method_name(&call)) || l.hook(method_name(&call))) {
            return;
        }
        // ignored_shared_example?: inside `(block (send #rspec? #SharedGroups.examples ...) ...)`
        if cx.cfg.bool("IgnoreSharedExamples") == Some(true)
            && cx.ancestors.iter().any(|a| {
                helpers::rspec::plain_block_call(a).is_some_and(|c| {
                    helpers::rspec::rspec_recv(&c) && l.sg_examples.iter().any(|s| s == method_name(&c))
                })
            })
        {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("always");
        if style != "always" && style != "named_only" {
            return;
        }
        // subject_usage: $(send nil? :subject), with the plain blocks around each one
        struct V<'pr> {
            blocks: Vec<CallNode<'pr>>,
            found: Vec<((usize, usize), Vec<CallNode<'pr>>)>,
        }
        impl<'pr> Visit<'pr> for V<'pr> {
            fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                // A plain block is an ancestor of its send and everything in it.
                let plain = helpers::rspec::plain_block_call(&n.as_node()).is_some();
                if plain {
                    self.blocks.push(n.as_node().as_call_node().unwrap());
                }
                if n.receiver().is_none() && method_name(n) == "subject" && send_args(n).is_empty() {
                    let sel = n.message_loc().unwrap();
                    let around: Vec<CallNode> =
                        self.blocks.iter().rev().map(|b| b.as_node().as_call_node().unwrap()).collect();
                    self.found.push(((sel.start_offset(), sel.end_offset()), around));
                }
                ruby_prism::visit_call_node(self, n);
                if plain {
                    self.blocks.pop();
                }
            }
        }
        let mut v = V { blocks: Vec::new(), found: Vec::new() };
        v.visit(node);
        for ((s, e), around) in v.found {
            if style == "named_only" {
                // nearest_subject: the first subject among the body children of the nearest block
                // ancestor (inside the example, then outside it) that has one
                let outer: Vec<CallNode> = cx.ancestors.iter().rev().filter_map(helpers::rspec::plain_block_call).collect();
                let named = around.iter().chain(outer.iter()).find_map(|b| {
                    let body = helpers::rspec::parser_body(helpers::rspec::block_body(b))?;
                    helpers::rspec::child_nodes(&body).into_iter().find(|c| helpers::rspec::subject(&l, c))
                });
                if !named.is_some_and(|n| !send_args(&n.as_call_node().unwrap()).is_empty()) {
                    continue;
                }
            }
            cx.add(s, e, "Name your test subject if you need to reference it explicitly.");
        }
    }
}
