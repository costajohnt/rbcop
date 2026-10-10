use crate::prelude::*;

pub const NAME: &str = "Style/WhileUntilDo";
pub static COP: WhileUntilDo = WhileUntilDo;
pub struct WhileUntilDo;

impl Cop for WhileUntilDo {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (keyword, do_loc, cond, body) = if let Some(w) = node.as_while_node() {
            ("while", w.do_keyword_loc(), w.predicate(), w.statements())
        } else {
            let u = node.as_until_node().unwrap();
            ("until", u.do_keyword_loc(), u.predicate(), u.statements())
        };
        let Some(do_loc) = do_loc else { return };
        let loc = node.location();
        if cx.src.line_of(loc.start_offset()) == cx.src.line_of(loc.end_offset()) {
            return;
        }
        if body.is_some_and(|b| cx.src.line_of(b.location().start_offset()) == cx.src.line_of(do_loc.start_offset())) {
            return;
        }
        let cond_end = cond.location().end_offset();
        let msg = format!("Do not use `do` with multi-line `{keyword}`.");
        cx.add_fix(do_loc.start_offset(), do_loc.end_offset(), msg, |c| c.remove(cond_end, do_loc.end_offset()));
    }
}
