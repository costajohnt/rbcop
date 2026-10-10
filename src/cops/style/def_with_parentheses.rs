use crate::prelude::*;

pub const NAME: &str = "Style/DefWithParentheses";
pub static COP: DefWithParentheses = DefWithParentheses;
pub struct DefWithParentheses;

impl Cop for DefWithParentheses {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let (Some(l), Some(r), None) = (def.lparen_loc(), def.rparen_loc(), def.parameters()) else { return };
        let (start, end) = (l.start_offset(), r.end_offset());
        // parentheses_required?
        let after = cx.src.text.as_bytes().get(end).copied();
        if after != Some(b';') {
            let loc = node.location();
            let single_line = cx.src.line_of(loc.start_offset()) == cx.src.line_of(loc.end_offset());
            if (single_line && def.equal_loc().is_none()) || after == Some(b'=') {
                return;
            }
        }
        cx.add_fix(start, end, "Omit the parentheses in defs when the method doesn't accept any arguments.", |c| {
            c.remove(start, end)
        });
    }
}
