use crate::prelude::*;

pub const NAME: &str = "RSpec/UndescriptiveLiteralsDescription";
pub static COP: UndescriptiveLiteralsDescription = UndescriptiveLiteralsDescription;
pub struct UndescriptiveLiteralsDescription;

impl Cop for UndescriptiveLiteralsDescription {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send #rspec? {#ExampleGroups.all #Examples.all} $_) ...)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        // NodePattern checks the arity (one argument) before it calls the predicates.
        let args = send_args(&call);
        let [arg] = &args[..] else { return };
        let l = helpers::rspec::lang(cx);
        let n = method_name(&call);
        if !helpers::rspec::rspec_recv(&call) || !(l.example_group(n) || l.example(n)) {
            return;
        }
        if matches!(
            kind::of(arg),
            kind::X_STRING_NODE
                | kind::INTERPOLATED_X_STRING_NODE
                | kind::INTEGER_NODE
                | kind::REGULAR_EXPRESSION_NODE
                | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
        ) {
            let (s, e) = helpers::rspec::node_range(arg);
            cx.add(s, e, "Description should be descriptive.");
        }
    }
}
