use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/AroundBlock";
pub static COP: AroundBlock = AroundBlock;
pub struct AroundBlock;

/// find_arg_usage: `{(send $... {:call :run}) (send _ _ $...) (yield $...) (block-pass $...)}`,
/// any of whose captures is `(lvar name)`.
fn used(node: &Node, name: &[u8]) -> bool {
    struct V<'a> {
        name: &'a [u8],
        found: bool,
    }
    impl V<'_> {
        fn lvar(&self, n: &Node) -> bool {
            n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == self.name)
                || (self.name == b"it" && n.as_it_local_variable_read_node().is_some())
        }
    }
    impl<'pr> Visit<'pr> for V<'_> {
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            if !is_csend(n) {
                let args = send_args(n);
                let call_run = args.is_empty() && matches!(method_name(n), "call" | "run");
                if call_run {
                    if n.receiver().is_some_and(|r| self.lvar(&r)) {
                        self.found = true;
                    }
                } else if args.iter().any(|a| self.lvar(a)) {
                    self.found = true;
                }
            }
            ruby_prism::visit_call_node(self, n);
        }
        fn visit_yield_node(&mut self, n: &ruby_prism::YieldNode<'pr>) {
            if n.arguments().is_some_and(|a| a.arguments().iter().any(|x| self.lvar(&x))) {
                self.found = true;
            }
            ruby_prism::visit_yield_node(self, n);
        }
        fn visit_block_argument_node(&mut self, n: &ruby_prism::BlockArgumentNode<'pr>) {
            if n.expression().is_some_and(|x| self.lvar(&x)) {
                self.found = true;
            }
            ruby_prism::visit_block_argument_node(self, n);
        }
    }
    let mut v = V { name, found: false };
    v.visit(node);
    v.found
}

impl Cop for AroundBlock {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :around sym ?)
        let call = node.as_call_node().unwrap();
        let Some(block) = helpers::rspec::block_of(&call) else { return };
        if call.receiver().is_some() || method_name(&call) != "around" {
            return;
        }
        match &send_args(&call)[..] {
            [] => {}
            [a] if helpers::rspec::is_sym(cx, a) => {}
            _ => return,
        }
        let unused = |name: &str| format!("You should call `{name}.call` or `{name}.run`.");
        match block.parameters() {
            Some(p) if p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some() => {
                let numbered = p.as_numbered_parameters_node().is_some();
                let var: &[u8] = if numbered { b"_1" } else { b"it" };
                if used(node, var) {
                    return;
                }
                // add_offense(block.children.last): the body (RuboCop raises without one)
                let Some(body) = block.body() else { return };
                let (s, e) = body_range(&body);
                cx.add(s, e, unused(if numbered { "_1" } else { "it" }));
            }
            params => {
                let first = params
                    .and_then(|p| p.as_block_parameters_node())
                    .and_then(|p| helpers::rspec::first_block_param(&p));
                match first {
                    None => {
                        let (s, e) = helpers::rspec::node_range(node);
                        cx.add(s, e, "Test object should be passed to around block.");
                    }
                    // `(a, b)` has no `name`: RuboCop raises.
                    Some((_, _, None)) => {}
                    Some((s, e, Some(name))) => {
                        if !used(node, name.as_bytes()) {
                            cx.add(s, e, unused(&name));
                        }
                    }
                }
            }
        }
    }
}
