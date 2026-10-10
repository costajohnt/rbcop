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
        // parser has no token for `__END__` (the data section is not lexed).
        let Some(token) = helpers::parser_tokens::sorted_tokens(src)
            .into_iter()
            .find(|t| t.kind != sys::PM_TOKEN_NEWLINE)
            .filter(|t| t.kind != sys::PM_TOKEN___END__)
        else {
            return;
        };
        if src.line_of(token.start) <= 1 {
            return;
        }
        // ponytail: Prism token ends differ from parser's for strings etc.; only comments are mapped.
        let end = src.comments.iter().find(|c| c.start == token.start).map_or(token.end, |c| c.end);
        cx.add(token.start, end, "Unnecessary blank line at the beginning of the source.");
    }
}
