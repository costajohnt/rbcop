use crate::prelude::*;

pub const NAME: &str = "RSpec/LeadingSubject";
pub static COP: LeadingSubject = LeadingSubject;
pub struct LeadingSubject;

fn offending(l: &helpers::rspec::Language, n: &Node) -> bool {
    use helpers::rspec::*;
    let_(l, n) || hook(l, n) || example(l, n) || spec_group(l, n) || include_(l, n)
}

impl Cop for LeadingSubject {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::subject(&l, node) || !helpers::rspec::inside_example_group(&l, cx.ancestors, node) {
            return;
        }
        // node.each_ancestor(:block).first.body (`block` only: numblocks and itblocks are skipped)
        let Some(block) = cx.ancestors.iter().rev().find_map(|a| helpers::rspec::plain_block_call(a)) else { return };
        let body = helpers::rspec::block_body(&block);
        // `body.each_child_node`: a `begin`'s statements, else the parts of the one statement
        let siblings = if helpers::rspec::is_begin_body(&body) {
            helpers::rspec::body_children(body)
        } else {
            helpers::rspec::parser_body(body).map(|s| helpers::rspec::child_nodes(&s)).unwrap_or_default()
        };
        // `break if sibling.equal?(node)`: a subject that is no sibling itself (`subject { } if c`)
        // never stops the walk, so a later example is an offender too (and a nil Language list
        // raises there)
        let key = helpers::rspec::node_range(node);
        let is_node = |s: &Node| s.as_call_node().is_some() && helpers::rspec::node_range(s) == key;
        // `find` stops at the first offender (its add_offense is truthy)
        let Some(offender) = siblings.iter().take_while(|s| !is_node(s)).find(|s| offending(&l, s)) else { return };
        let name = method_name(&offender.as_call_node().unwrap()).to_string();
        let anc = cx.ancestors;
        let Some(mv) = helpers::rspec::MoveNode::new(cx.src, &anc[0], node, offender, false) else { return };
        let (s, e) = helpers::rspec::node_range(node);
        cx.add_fix(s, e, format!("Declare `subject` above any other `{name}` declarations."), |c| mv.apply(c));
    }
}
