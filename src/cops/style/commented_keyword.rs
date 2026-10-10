use crate::helpers::range_help::{Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/CommentedKeyword";
pub static COP: CommentedKeyword = CommentedKeyword;
pub struct CommentedKeyword;

// Ruby's `\s` and `\w` are ASCII-only.
static KEYWORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t\r\n\f\v]*(?:begin|class|def|end|module)[ \t\r\n\f\v]").unwrap());
static ALLOWED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"#[ \t\r\n\f\v]*:nodoc:|#[ \t\r\n\f\v]*:yields:|#[ \t\r\n\f\v]*rubocop[ \t\r\n\f\v]*:[ \t\r\n\f\v]*(?:disable-next|enable-next|todo-next|disable|enable|todo|push|pop|next)\b",
    )
    .unwrap()
});
static MATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([^ \t\r\n\f\v]+).*#").unwrap());
static SUBCLASS_DEFINITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A[ \t\r\n\f\v]*class[ \t\r\n\f\v]+(?:[A-Za-z0-9_]|::)+[ \t\r\n\f\v]*<[ \t\r\n\f\v]*(?:[A-Za-z0-9_]|::)+")
        .unwrap()
});
static METHOD_OR_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[ \t\r\n\f\v]*(?:def[ \t\r\n\f\v]|end)").unwrap());
static RBS_BRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A#\[.+\]").unwrap());
static STEEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#[ \t\r\n\f\v]steep:ignore(?:[ \t\r\n\f\v]|\z)").unwrap());

impl Cop for CommentedKeyword {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        for c in &src.comments {
            let ln = src.line_of(c.start);
            let line_start = src.line_start(ln);
            let line_end = src.text[line_start..].find('\n').map_or(src.text.len(), |i| line_start + i);
            let line = &src.text[line_start..line_end];
            let text = src.slice(c.start, c.end);
            let rbs = if SUBCLASS_DEFINITION.is_match(line) {
                RBS_BRACKET.is_match(text)
            } else if METHOD_OR_END.is_match(line) {
                text.starts_with("#:")
            } else {
                false
            };
            if rbs || STEEP.is_match(text) || !KEYWORD.is_match(line) || ALLOWED.is_match(line) {
                continue;
            }
            let Some(m) = MATCH.captures(line) else { continue };
            let keyword = m[1].to_string();
            let (start, end) = (c.start, c.end);
            let range = range_with_surrounding_space(src.text, start, end, Surrounding { newlines: false, ..Default::default() });
            let msg = format!("Do not place comments on the same line as the `{keyword}` keyword.");
            cx.add_fix(start, end, msg, |corr| {
                corr.remove(range.0, range.1);
                if keyword != "end" {
                    corr.insert_before(line_start, line_end, &format!("{text}\n"));
                }
            });
        }
    }
}
