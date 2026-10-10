use crate::prelude::*;

pub const NAME: &str = "Style/InlineComment";
pub static COP: InlineComment = InlineComment;
pub struct InlineComment;

static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A# rubocop:(enable|disable|todo)").unwrap());

impl Cop for InlineComment {
    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        for c in &src.comments {
            // comment_line?: /^\s*#/ on the comment's line
            let line = src.lines.get(src.line_of(c.start) - 1).copied().unwrap_or("");
            let comment_line = line.trim_start_matches([' ', '\t', '\r', '\n', '\x0c', '\x0b']).starts_with('#');
            if comment_line || DIRECTIVE.is_match(src.slice(c.start, c.end)) {
                continue;
            }
            cx.add(c.start, c.end, "Avoid trailing inline comments.");
        }
    }
}
