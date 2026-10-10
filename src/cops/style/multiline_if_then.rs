use crate::prelude::*;
use helpers::if_node::{normal_keyword, parser_end};
use helpers::range_help::{range_with_surrounding_space, Side, Surrounding};

pub const NAME: &str = "Style/MultilineIfThen";
pub static COP: MultilineIfThen = MultilineIfThen;
pub struct MultilineIfThen;

impl Cop for MultilineIfThen {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let Some(keyword) = normal_keyword(node) else { return };
        let (then, body) = match node.as_if_node() {
            Some(n) => (n.then_keyword_loc(), n.statements()),
            None => {
                let n = node.as_unless_node().unwrap();
                (n.then_keyword_loc(), n.statements())
            }
        };
        // An `elsif` without its own `then` gets the previous branch's (outside the node).
        let start = node.location().start_offset();
        let Some(then) = then.filter(|t| t.as_slice() == b"then" && t.start_offset() >= start) else { return };
        let src = cx.src;
        if src.line_of(node.location().start_offset()) == src.line_of(parser_end(node)) {
            return;
        }
        let (s, e) = (then.start_offset(), then.end_offset());
        if body.is_some_and(|b| src.line_of(b.location().start_offset()) == src.line_of(s)) {
            return;
        }
        let opts = Surrounding { side: Side::Left, ..Default::default() };
        let (rs, re) = range_with_surrounding_space(src.text, s, e, opts);
        cx.add_fix(s, e, format!("Do not use `then` for multi-line `{keyword}`."), |c| c.remove(rs, re));
    }
}
