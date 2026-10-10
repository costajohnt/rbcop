use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLines";
pub static COP: EmptyLines = EmptyLines;
pub struct EmptyLines;

impl Cop for EmptyLines {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let no_tokens = cx.facts.first_code.is_none() && src.comments.is_empty();
        if no_tokens || !src.text.contains("\n\n\n") {
            return;
        }
        // A blank line is extra when the line before it is blank too and the parser gem emits
        // no token on it (string/regexp content gets one token per line).
        let last_code = src.lines.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |i| i + 1);
        let last_comment = src.comments.last().map_or(0, |c| src.line_of(c.end - 1));
        let last_token_line = last_code.max(last_comment);
        for (i, pair) in src.lines.windows(2).enumerate().take(last_token_line.saturating_sub(2)) {
            let line = i + 2;
            if pair[0].is_empty() && pair[1].is_empty() && !cx.facts.string_lines[line] {
                let start = src.line_start(line);
                cx.add_fix(start, start + 1, "Extra blank line detected.", |c| c.remove(start, start + 1));
            }
        }
    }
}
