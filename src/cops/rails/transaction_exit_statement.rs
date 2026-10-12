use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/TransactionExitStatement";
pub static COP: TransactionExitStatement = TransactionExitStatement;
pub struct TransactionExitStatement;

struct Exits<'c> {
    is_tx: &'c dyn Fn(&str) -> bool,
    /// Method names of the enclosing parser blocks (innermost last).
    blocks: Vec<String>,
    found: Vec<(usize, usize, &'static str)>,
}

impl<'pr> Visit<'pr> for Exits<'_> {
    fn visit_return_node(&mut self, n: &ruby_prism::ReturnNode<'pr>) {
        self.found.push((n.location().start_offset(), n.location().end_offset(), "return"));
        ruby_prism::visit_return_node(self, n);
    }
    fn visit_break_node(&mut self, n: &ruby_prism::BreakNode<'pr>) {
        // nested_block?: the nearest enclosing block's method is not a transaction method.
        let nested = self.blocks.last().is_some_and(|m| !(self.is_tx)(m));
        if !nested {
            self.found.push((n.location().start_offset(), n.location().end_offset(), "break"));
        }
        ruby_prism::visit_break_node(self, n);
    }
    fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
        if !is_csend(c) && c.receiver().is_none() && method_name(c) == "throw" {
            let (s, e) = send_range(c);
            self.found.push((s, e, "throw"));
        }
        if let Some(r) = c.receiver() {
            self.visit(&r);
        }
        if let Some(a) = c.arguments() {
            self.visit_arguments_node(&a);
        }
        if let Some(b) = c.block() {
            let block = b.as_block_node().is_some();
            if block {
                self.blocks.push(method_name(c).to_string());
            }
            self.visit(&b);
            if block {
                self.blocks.pop();
            }
        }
    }
    fn visit_lambda_node(&mut self, l: &ruby_prism::LambdaNode<'pr>) {
        self.blocks.push("lambda".to_string());
        ruby_prism::visit_lambda_node(self, l);
        self.blocks.pop();
    }
    fn visit_super_node(&mut self, n: &ruby_prism::SuperNode<'pr>) {
        let block = n.block().is_some_and(|b| b.as_block_node().is_some());
        if block {
            self.blocks.push("super".to_string());
        }
        ruby_prism::visit_super_node(self, n);
        if block {
            self.blocks.pop();
        }
    }
    fn visit_forwarding_super_node(&mut self, n: &ruby_prism::ForwardingSuperNode<'pr>) {
        let block = n.block().is_some();
        if block {
            self.blocks.push("super".to_string());
        }
        ruby_prism::visit_forwarding_super_node(self, n);
        if block {
            self.blocks.pop();
        }
    }
}

impl Cop for TransactionExitStatement {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if cx.config.target_rails_version() >= 7.2 {
            return;
        }
        let Some(call) = helpers::rails::send(node) else { return };
        let extra = cx.cfg.strings("TransactionMethods");
        let is_tx = |m: &str| matches!(m, "transaction" | "with_lock") || extra.iter().any(|x| x == m);
        if !is_tx(method_name(&call)) {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let Some(body) = block.body() else { return };
        // right_siblings: the block's body must not be a loop.
        let stmts = helpers::rails::body_statements(Some(body));
        if let [only] = stmts.as_slice()
            && matches!(only, Node::WhileNode { .. } | Node::UntilNode { .. } | Node::ForNode { .. })
        {
            return;
        }
        let mut ex = Exits { is_tx: &is_tx, blocks: vec![method_name(&call).to_string()], found: Vec::new() };
        ex.visit(&block.body().unwrap());
        for (s, e, st) in ex.found {
            cx.add(s, e, format!("Exit statement `{st}` is not allowed. Use `raise` (rollback) or `next` (commit)."));
        }
    }
}
