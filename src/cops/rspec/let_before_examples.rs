use crate::prelude::*;

pub const NAME: &str = "RSpec/LetBeforeExamples";
pub static COP: LetBeforeExamples = LetBeforeExamples;
pub struct LetBeforeExamples;

/// `{(block (send nil? {#ExampleGroups.all #Examples.all} ...) ...) (send nil? #Includes.examples ...)}`
fn example_or_group(l: &helpers::rspec::Language, node: &Node) -> bool {
    if let Some(c) = helpers::rspec::plain_block_call(node) {
        let n = method_name(&c);
        return c.receiver().is_none() && (l.example_group(n) || l.example(n));
    }
    helpers::rspec::send_call(node).is_some_and(|c| {
        c.receiver().is_none() && l.inc_examples.has(method_name(&c))
    })
}

/// `{(block (send nil? :include_examples ...) ...) (send nil? :include_examples ...)}`
fn include_examples(node: &Node) -> bool {
    let Some(c) = node.as_call_node() else { return false };
    c.receiver().is_none()
        && method_name(&c) == "include_examples"
        && helpers::rspec::block_of(&c).is_none_or(|b| helpers::rspec::is_plain(&b))
}

impl Cop for LetBeforeExamples {
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
        let correct = !stmts.iter().any(include_examples);
        let anc = cx.ancestors;
        let program = &anc[0];
        for sibling in &stmts[first + 1..] {
            if !helpers::rspec::let_(&l, sibling) {
                continue;
            }
            let (s, e) = helpers::rspec::node_range(sibling);
            let mv = if correct { helpers::rspec::MoveNode::new(cx.src, program, sibling, &stmts[first], false) } else { None };
            // MoveNode raises (a `let` send without `loc.end`): the offense and the rest are lost.
            if correct && mv.is_none() {
                break;
            }
            cx.add_fix(s, e, "Move `let` before the examples in the group.", |c| {
                if let Some(mv) = &mv {
                    mv.apply(c);
                }
            });
        }
    }
}
