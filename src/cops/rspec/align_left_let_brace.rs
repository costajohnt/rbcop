use crate::prelude::*;

pub const NAME: &str = "RSpec/AlignLeftLetBrace";
pub static COP: AlignLeftLetBrace = AlignLeftLetBrace;
pub struct AlignLeftLetBrace;

impl Cop for AlignLeftLetBrace {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        for ((s, e), indent) in helpers::rspec::align_let_brace(cx, node, true) {
            let spaces = " ".repeat(indent);
            cx.add_fix(s, e, "Align left let brace", |c| c.insert_before(s, e, &spaces));
        }
    }
}
