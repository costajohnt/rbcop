use crate::prelude::*;

pub const NAME: &str = "RSpec/AlignRightLetBrace";
pub static COP: AlignRightLetBrace = AlignRightLetBrace;
pub struct AlignRightLetBrace;

impl Cop for AlignRightLetBrace {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        for ((s, e), indent) in helpers::rspec::align_let_brace(cx, node, false) {
            let spaces = " ".repeat(indent);
            cx.add_fix(s, e, "Align right let brace", |c| c.insert_before(s, e, &spaces));
        }
    }
}
