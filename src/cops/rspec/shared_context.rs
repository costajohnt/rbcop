use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/SharedContext";
pub static COP: SharedContext = SharedContext;
pub struct SharedContext;

/// (examples?, context?): `(send nil? {#Includes.examples #Examples.all} ...)` and
/// `(send nil? {#Subjects.all #Helpers.all #Includes.context #Hooks.all} ...)` in the node.
fn search(l: &helpers::rspec::Language, node: &Node) -> (bool, bool) {
    struct V<'a> {
        l: &'a helpers::rspec::Language,
        examples: bool,
        context: bool,
    }
    impl<'pr> Visit<'pr> for V<'_> {
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            if n.receiver().is_none() {
                let m = method_name(n);
                self.examples |= self.l.inc_examples.iter().any(|x| x == m) || self.l.example(m);
                self.context |= self.l.subject(m) || self.l.helper(m) || self.l.inc_context.iter().any(|x| x == m) || self.l.hook(m);
            }
            ruby_prism::visit_call_node(self, n);
        }
    }
    let mut v = V { l, examples: false, context: false };
    v.visit(node);
    (v.examples, v.context)
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
        let shared_context = l.sg_context.iter().any(|x| x == name);
        let shared_examples = l.sg_examples.iter().any(|x| x == name);
        if !shared_context && !shared_examples {
            return;
        }
        let (examples, context) = search(&l, node);
        let strict = cx.cfg.bool("Strict") == Some(true);
        let (s, e) = send_range(&call);
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        if shared_context && examples && (strict || !context) {
            let msg = if strict {
                "Use `shared_examples` when you define examples."
            } else {
                "Use `shared_examples` when you don't define context."
            };
            let fix = !strict || !context;
            cx.add_fix(s, e, msg, |c| {
                if fix {
                    c.replace(ss, se, "shared_examples");
                }
            });
        }
        if shared_examples && context && !examples {
            cx.add_fix(s, e, "Use `shared_context` when you don't define examples.", |c| {
                c.replace(ss, se, "shared_context")
            });
        }
    }
}
