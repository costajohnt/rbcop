use crate::prelude::*;

pub const NAME: &str = "RSpec/MultipleSubjects";
pub static COP: MultipleSubjects = MultipleSubjects;
pub struct MultipleSubjects;

impl Cop for MultipleSubjects {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if helpers::rspec::plain_block_call(node).is_none() || !helpers::rspec::example_group(&l, node) {
            return;
        }
        let subjects = helpers::rspec::find_all_in_scope(&l, node, helpers::rspec::subject);
        let Some((_, rest)) = subjects.split_last() else { return };
        for subject in rest {
            let call = subject.as_call_node().unwrap();
            let (s, e) = helpers::rspec::node_range(subject);
            // `subject!` is left alone; a named subject becomes a `let`, an unnamed one goes
            let edit = if method_name(&call) != "subject" {
                None
            } else if !send_args(&call).is_empty() {
                let sel = call.message_loc().unwrap();
                Some((sel.start_offset(), sel.end_offset(), "let"))
            } else {
                let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true);
                Some((rs, re, ""))
            };
            cx.add_fix(s, e, "Do not set more than one subject per example group", |c| {
                if let Some((a, b, t)) = edit {
                    c.replace(a, b, t);
                }
            });
        }
    }
}
