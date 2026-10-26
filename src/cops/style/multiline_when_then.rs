use crate::prelude::*;
use helpers::range_help::{range_with_surrounding_space, Side, Surrounding};

pub const NAME: &str = "Style/MultilineWhenThen";
pub static COP: MultilineWhenThen = MultilineWhenThen;
pub struct MultilineWhenThen;

impl Cop for MultilineWhenThen {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::WHEN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let w = node.as_when_node().unwrap();
        let Some(then) = w.then_keyword_loc().filter(|t| t.as_slice() == b"then") else { return };
        let conds: Vec<Node> = w.conditions().iter().collect();
        let (Some(first), Some(last)) = (conds.first(), conds.last()) else { return };
        let src = cx.src;
        // require_then?
        if src.line_of(first.location().start_offset()) != src.line_of(last.location().end_offset()) {
            return;
        }
        // 1.92: `then` is required after an endless range (`when 1.. then`).
        if ends_with_endless_range(src, last) {
            return;
        }
        if let Some(body) = w.statements()
            && src.line_of(node.location().start_offset()) == src.line_of(body.location().start_offset()) {
                return;
            }
        let (s, e) = (then.start_offset(), then.end_offset());
        let opts = Surrounding { side: Side::Left, newlines: false, ..Default::default() };
        let (rs, re) = range_with_surrounding_space(src.text, s, e, opts);
        cx.add_fix(s, e, "Do not use `then` for multiline `when` statement.", |c| c.remove(rs, re));
    }
}

/// `ends_with_endless_range?`: an endless range ends where `node` ends.
// ponytail: only an endless range's `..`/`...` can end a node's source with `..`.
pub fn ends_with_endless_range(src: &crate::source::Source, node: &Node) -> bool {
    src.slice(node.location().start_offset(), node.location().end_offset()).ends_with("..")
}
