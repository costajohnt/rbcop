use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLineAfterMagicComment";
pub static COP: EmptyLineAfterMagicComment = EmptyLineAfterMagicComment;
pub struct EmptyLineAfterMagicComment;

impl Cop for EmptyLineAfterMagicComment {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let code_line = cx.facts.first_code.map(|off| src.line_of(off));
        let Some(magic) = src
            .comments
            .iter()
            .take_while(|c| code_line.is_none_or(|l| src.line_of(c.start) < l))
            .filter(|c| helpers::magic_comment::any(src.slice(c.start, c.end)))
            .last()
        else {
            return;
        };
        let expected = cx.cfg.int("NumberOfEmptyLines").unwrap_or(1);
        // processed_source[i] is the 0-based line i; the magic comment's own line number is the next line's index.
        let first = src.line_of(magic.start);
        let line = |i: usize| src.lines.get(i);
        let mut actual = 0;
        while line(first + actual).is_some_and(|l| helpers::ruby_string::strip(l).is_empty()) {
            actual += 1;
        }
        if line(first + actual).is_none() || actual as i64 >= expected {
            return;
        }
        let start = src.line_start(first + 1);
        let msg = format!(
            "Expected at least {expected} empty {} after magic comments; found {actual}.",
            if expected == 1 { "line" } else { "lines" }
        );
        // source_range(buffer, line, 0): one character.
        let len = src.text[start..].chars().next().map_or(0, char::len_utf8);
        cx.add(start, start + len, msg);
    }
}
