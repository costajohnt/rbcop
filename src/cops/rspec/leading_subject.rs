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
        // `each_child_node` of a non-`begin` body are the parts of one statement: never offending
        // in practice, and never the subject itself.
        if !helpers::rspec::is_begin_body(&body) {
            return;
        }
        let start = node.location().start_offset();
        let stmts = helpers::rspec::body_children(body);
        let Some(offender) = stmts.iter().take_while(|s| s.location().start_offset() != start).find(|s| offending(&l, s))
        else {
            return;
        };
        let name = method_name(&offender.as_call_node().unwrap()).to_string();
        let anc = cx.ancestors;
        let Some(mv) = helpers::rspec::MoveNode::new(cx.src, &anc[0], node, offender, false) else { return };
        let (s, e) = helpers::rspec::node_range(node);
        cx.add_fix(s, e, format!("Declare `subject` above any other `{name}` declarations."), |c| mv.apply(c));
    }
}
