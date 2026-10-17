use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "FactoryBot/FactoryAssociationWithStrategy";
pub static COP: FactoryAssociationWithStrategy = FactoryAssociationWithStrategy;
pub struct FactoryAssociationWithStrategy;

impl Cop for FactoryAssociationWithStrategy {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // factory_declaration: (block (send nil? {:factory :trait} ...) ...)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if call.receiver().is_some() || !matches!(method_name(&call), "factory" | "trait") {
            return;
        }
        // factory_strategy_association on each node below: (block (send nil? _) (args)
        // <$(send nil? {:create :build :build_stubbed} ...) ...>), the strategy call a direct child
        struct V(Vec<(usize, usize)>);
        impl<'pr> Visit<'pr> for V {
            fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                if let Some(block) = helpers::rspec::block_of(n).filter(helpers::rspec::is_plain)
                    && n.receiver().is_none()
                    && send_args(n).is_empty()
                    && !is_csend(n)
                    && block.parameters().is_none_or(|p| {
                        p.as_block_parameters_node().is_some_and(|b| b.parameters().is_none() && b.locals().iter().next().is_none())
                    })
                    && let Some(body) = helpers::rspec::parser_body(block.body())
                    && body.as_statements_node().is_none()
                    && let Some(s) = helpers::rspec::send_call(&body)
                    && s.receiver().is_none()
                    && matches!(method_name(&s), "create" | "build" | "build_stubbed")
                {
                    self.0.push(send_range(&s));
                }
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V(Vec::new());
        v.visit(node);
        for (s, e) in v.0 {
            cx.add(
                s,
                e,
                "Use an implicit, explicit or inline definition instead of hard coding a strategy for setting association within factory.",
            );
        }
    }
}
