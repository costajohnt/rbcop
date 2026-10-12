use crate::prelude::*;

pub const NAME: &str = "Rails/ActiveRecordCallbacksOrder";
pub static COP: ActiveRecordCallbacksOrder = ActiveRecordCallbacksOrder;
pub struct ActiveRecordCallbacksOrder;

const ORDER: &[&str] = &[
    "after_initialize", "before_validation", "after_validation", "before_save", "around_save", "before_create",
    "around_create", "after_create", "before_update", "around_update", "after_update", "before_destroy",
    "around_destroy", "after_destroy", "after_save", "after_commit", "after_rollback", "after_find", "after_touch",
];

fn callback_index(n: &Node) -> Option<usize> {
    let c = helpers::rails::send_node(n)?;
    ORDER.iter().position(|m| *m == method_name(&c))
}

/// source_range_with_comment: from the newline before the node's line (or before the own-line
/// comments directly above it) to the end of its last line.
fn range_with_comment(src: &crate::source::Source, s: usize, e: usize) -> (usize, usize) {
    let mut line = src.line_of(s);
    let mut annotation = line - 1;
    // each_comment_in_lines(0..annotation_line).reverse_each
    let comments: Vec<&Comment> = src.comments.iter().filter(|c| src.line_of(c.start) <= annotation).collect();
    for c in comments.iter().rev() {
        let cl = src.line_of(c.start);
        let own_line = src.slice(src.line_start(cl), c.start).trim().is_empty();
        if cl == annotation && own_line {
            line = cl;
            annotation -= 1;
        }
    }
    let begin = src.line_start(line).saturating_sub(1);
    let last = src.line_of(e);
    let end = src.line_start(last) + src.lines[last - 1].len();
    (begin, end)
}

impl Cop for ActiveRecordCallbacksOrder {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        // class_def.each_child_node: only a multi-statement body has statements as children.
        let stmts = helpers::rails::body_statements(class.body());
        if stmts.len() < 2 {
            return;
        }
        let src = cx.src;
        let mut previous: Option<usize> = None;
        let mut prev_index: isize = -1;
        let mut last_cb: Option<usize> = None;
        for (i, st) in stmts.iter().enumerate() {
            let Some(index) = callback_index(st) else { continue };
            if (index as isize) < prev_index {
                let call = helpers::rails::send_node(st).unwrap();
                let msg = format!(
                    "`{}` is supposed to appear before `{}`.",
                    method_name(&call),
                    ORDER[previous.unwrap()]
                );
                let (s, e) = (st.location().start_offset(), st.location().end_offset());
                let p = &stmts[last_cb.unwrap()];
                let cur = range_with_comment(src, s, e);
                let prev = range_with_comment(src, p.location().start_offset(), p.location().end_offset());
                let moved = src.slice(cur.0, cur.1).to_string();
                cx.add_fix(s, e, msg, |c| {
                    c.insert_before(prev.0, prev.1, &moved);
                    c.remove(cur.0, cur.1);
                });
            }
            prev_index = index as isize;
            previous = Some(index);
            last_cb = Some(i);
        }
    }
}
