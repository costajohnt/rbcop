use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceAfterMethodName";
pub static COP: SpaceAfterMethodName = SpaceAfterMethodName;
pub struct SpaceAfterMethodName;

impl Cop for SpaceAfterMethodName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(lparen) = node.as_def_node().unwrap().lparen_loc() else { return };
        let pos = lparen.start_offset();
        if pos > 0 && cx.src.text.as_bytes()[pos - 1] == b' ' {
            cx.add_fix(pos - 1, pos, "Do not put a space between a method name and the opening parenthesis.", |c| c.remove(pos - 1, pos));
        }
    }
}
