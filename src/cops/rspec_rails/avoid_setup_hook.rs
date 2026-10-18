use crate::prelude::*;

pub const NAME: &str = "RSpecRails/AvoidSetupHook";
pub static COP: AvoidSetupHook = AvoidSetupHook;
pub struct AvoidSetupHook;

impl Cop for AvoidSetupHook {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        // inside_example_group? is asked of every block, before setup_call
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::inside_example_group(&l, cx.ancestors, node) {
            return;
        }
        // (block $(send nil? :setup) (args) _)
        if call.receiver().is_some() || method_name(&call) != "setup" || !send_args(&call).is_empty() {
            return;
        }
        let block = helpers::rspec::block_of(&call).unwrap();
        if block.parameters().is_some_and(|p| {
            p.as_block_parameters_node().is_none_or(|b| b.parameters().is_some() || b.locals().iter().next().is_some())
        }) {
            return;
        }
        let (ss, se) = send_range(&call);
        let (s, e) = helpers::rspec::node_range(node);
        cx.add_fix(s, e, "Use `before` instead of `setup`.", |c| c.replace(ss, se, "before"));
    }
}
