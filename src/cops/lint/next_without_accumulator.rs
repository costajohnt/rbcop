use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/NextWithoutAccumulator";
pub static COP: NextWithoutAccumulator = NextWithoutAccumulator;
pub struct NextWithoutAccumulator;

/// First argument-less `next` in source order whose nearest parser block is the one searched:
/// a nested block (with its whole send) or lambda hides its `next`s.
#[derive(Default)]
struct FindNext {
    found: Option<(usize, usize)>,
}

impl<'pr> Visit<'pr> for FindNext {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if node.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'pr>) {
        if node.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        ruby_prism::visit_super_node(self, node);
    }

    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode<'pr>) {
        if node.block().is_none() {
            ruby_prism::visit_forwarding_super_node(self, node);
        }
    }

    fn visit_lambda_node(&mut self, _: &ruby_prism::LambdaNode<'pr>) {}

    fn visit_next_node(&mut self, node: &ruby_prism::NextNode<'pr>) {
        if node.arguments().is_none() && self.found.is_none() {
            let loc = node.location();
            self.found = Some((loc.start_offset(), loc.end_offset()));
        }
        ruby_prism::visit_next_node(self, node);
    }
}

impl Cop for NextWithoutAccumulator {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (call _recv {:reduce :inject} !sym) _ $(begin ...))
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if !matches!(method_name(&call), "reduce" | "inject") {
            return;
        }
        let args = send_args(&call);
        if args.len() != 1 || args[0].as_symbol_node().is_some() {
            return;
        }
        // Parser `begin` body: several statements, or one parenthesized expression.
        let Some(body) = block.body().and_then(|b| b.as_statements_node()) else { return };
        let stmts: Vec<Node> = body.body().iter().collect();
        if stmts.len() < 2 && !stmts.first().is_some_and(|s| s.as_parentheses_node().is_some()) {
            return;
        }
        let mut f = FindNext::default();
        f.visit_statements_node(&body);
        if let Some((s, e)) = f.found {
            cx.add(s, e, "Use `next` with an accumulator argument in a `reduce`.");
        }
    }
}
