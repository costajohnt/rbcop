use crate::prelude::*;

pub const NAME: &str = "RSpec/HooksBeforeExamples";
pub static COP: HooksBeforeExamples = HooksBeforeExamples;
pub struct HooksBeforeExamples;

/// `{(any_block {(send #rspec? #ExampleGroups.all ...) (send nil? #Examples.all ...)} ...)
/// (send nil? #Includes.examples ...)}`
fn example_or_group(l: &helpers::rspec::Language, node: &Node) -> bool {
    if let Some(c) = helpers::rspec::any_block_call(node) {
        let n = method_name(&c);
        return (helpers::rspec::rspec_recv(&c) && l.example_group(n)) || (c.receiver().is_none() && l.example(n));
    }
    helpers::rspec::send_call(node)
        .is_some_and(|c| c.receiver().is_none() && l.inc_examples.iter().any(|i| i == method_name(&c)))
}

impl Cop for HooksBeforeExamples {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::example_group_with_body(&l, node) {
            return;
        }
        let body = helpers::rspec::block_body(&node.as_call_node().unwrap());
        if !helpers::rspec::is_begin_body(&body) {
            return;
        }
        let stmts = helpers::rspec::body_children(body);
        let Some(first) = stmts.iter().position(|s| example_or_group(&l, s)) else { return };
        let anc = cx.ancestors;
        let program = &anc[0];
        for sibling in &stmts[first + 1..] {
            if !helpers::rspec::hook(&l, sibling) {
                continue;
            }
            let (s, e) = helpers::rspec::node_range(sibling);
            let name = method_name(&sibling.as_call_node().unwrap());
            let Some(mv) = helpers::rspec::MoveNode::new(cx.src, program, sibling, &stmts[first], false) else { break };
            cx.add_fix(s, e, format!("Move `{name}` above the examples in the group."), |c| mv.apply(c));
        }
    }
}
