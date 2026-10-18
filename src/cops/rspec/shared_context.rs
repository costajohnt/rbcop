use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/SharedContext";
pub static COP: SharedContext = SharedContext;
pub struct SharedContext;

/// def_node_search: a receiverless send in the node (itself included) whose name `f` takes,
/// asked in order until one is found.
fn search(node: &Node, f: &dyn Fn(&str) -> bool) -> bool {
    struct V<'a> {
        f: &'a dyn Fn(&str) -> bool,
        found: bool,
    }
    impl<'pr> Visit<'pr> for V<'_> {
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            if self.found {
                return;
            }
            self.found = n.receiver().is_none() && (self.f)(method_name(n));
            ruby_prism::visit_call_node(self, n);
        }
    }
    let mut v = V { f, found: false };
    v.visit(node);
    v.found
}

impl Cop for SharedContext {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if !helpers::rspec::rspec_recv(&call) {
            return;
        }
        let l = helpers::rspec::lang(cx);
        let name = method_name(&call);
        // shared_context asks SharedGroups.context, and shared_example (SharedGroups.examples)
        // only after the first offense is added
        let shared_context = l.sg_context.has(name);
        // examples?: (send nil? {#Includes.examples #Examples.all} ...); context?: (send nil?
        // {#Subjects.all #Helpers.all #Includes.context #Hooks.all} ...). Each is searched only when
        // on_block's conditions reach it (a Language list set to `~` raises when asked).
        let examples = || search(node, &|m| l.inc_examples.has(m) || l.example(m));
        let context = || search(node, &|m| l.subject(m) || l.helper(m) || l.inc_context.has(m) || l.hook(m));
        let strict = cx.cfg.bool("Strict") == Some(true);
        let (s, e) = send_range(&call);
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        if shared_context && examples() && (strict || !context()) {
            let msg = if strict {
                "Use `shared_examples` when you define examples."
            } else {
                "Use `shared_examples` when you don't define context."
            };
            let fix = !strict || !context();
            cx.add_fix(s, e, msg, |c| {
                if fix {
                    c.replace(ss, se, "shared_examples");
                }
            });
        }
        if l.sg_examples.has(name) && context() && !examples() {
            cx.add_fix(s, e, "Use `shared_context` when you don't define examples.", |c| {
                c.replace(ss, se, "shared_context")
            });
        }
    }
}
