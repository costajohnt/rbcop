use crate::prelude::*;

pub const NAME: &str = "Style/NegatedWhile";
pub static COP: NegatedWhile = NegatedWhile;
pub struct NegatedWhile;

impl Cop for NegatedWhile {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `begin ... end while x` is while_post in parser, which this cop does not handle.
        let (post, predicate, kw, inverse, current) = match node.as_while_node() {
            Some(w) => (w.is_begin_modifier(), w.predicate(), w.keyword_loc(), "until", "while"),
            None => {
                let u = node.as_until_node().unwrap();
                (u.is_begin_modifier(), u.predicate(), u.keyword_loc(), "while", "until")
            }
        };
        if !post && helpers::negative_conditional::single_negative(predicate) {
            let loc = node.location();
            let keyword = (kw.start_offset(), kw.end_offset());
            let pred = match node.as_while_node() {
                Some(w) => w.predicate(),
                None => node.as_until_node().unwrap().predicate(),
            };
            cx.add_fix(loc.start_offset(), loc.end_offset(), helpers::negative_conditional::message(inverse, current), |c| {
                helpers::negative_conditional::correct_negative_condition(c, keyword, inverse, pred)
            });
        }
    }
}
