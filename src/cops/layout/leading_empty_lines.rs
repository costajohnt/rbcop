use crate::prelude::*;

pub const NAME: &str = "Layout/LeadingEmptyLines";
pub static COP: LeadingEmptyLines = LeadingEmptyLines;
pub struct LeadingEmptyLines;

impl Cop for LeadingEmptyLines {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        // Parser-shaped tokens: one tSTRING for a simple string, comments without their newline.
        let Some(token) = helpers::parser_tokens::translated(src).into_iter().find(|t| t.kind != sys::PM_TOKEN_NEWLINE)
        else {
            return;
        };
        if src.line_of(token.start) <= 1 {
            return;
        }
        let (first, end) = (token.start, token.end);
        cx.add_fix(token.start, end, "Unnecessary blank line at the beginning of the source.", |c| c.remove(0, first));
    }
}
