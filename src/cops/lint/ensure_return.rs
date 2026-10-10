use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/EnsureReturn";
pub static COP: EnsureReturn = EnsureReturn;
pub struct EnsureReturn;

/// `each_node(:return)` minus `return_from_inner_scope?`: returns not inside a def or lambda.
#[derive(Default)]
struct Returns(Vec<(usize, usize)>);

impl<'pr> Visit<'pr> for Returns {
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode<'pr>) {
        let l = node.location();
        self.0.push((l.start_offset(), l.end_offset()));
        ruby_prism::visit_return_node(self, node);
    }
    fn visit_def_node(&mut self, _: &ruby_prism::DefNode<'pr>) {}
    fn visit_lambda_node(&mut self, _: &ruby_prism::LambdaNode<'pr>) {}
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if method_name(node) == "lambda" && node.block().is_some_and(|b| b.as_block_node().is_some()) {
            if let Some(r) = node.receiver() {
                self.visit(&r);
            }
            if let Some(a) = node.arguments() {
                self.visit_arguments_node(&a);
            }
            return;
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for EnsureReturn {
    fn kinds(&self) -> &'static [usize] {
        &[kind::ENSURE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(body) = node.as_ensure_node().unwrap().statements() else { return };
        let mut v = Returns::default();
        v.visit_statements_node(&body);
        for (s, e) in v.0 {
            cx.add(s, e, "Do not return from an `ensure` block.");
        }
    }
}
