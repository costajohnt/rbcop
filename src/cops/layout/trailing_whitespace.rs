use crate::prelude::*;

pub const NAME: &str = "Layout/TrailingWhitespace";
pub static COP: TrailingWhitespace = TrailingWhitespace;
pub struct TrailingWhitespace;

/// Ruby's [[:blank:]]: tab plus Unicode space separators.
pub fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

impl Cop for TrailingWhitespace {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let allow_in_heredoc = cx.cfg.bool("AllowInHeredoc").unwrap_or(false);
        for (i, line) in cx.src.lines.iter().enumerate() {
            let trimmed = line.trim_end_matches(is_blank);
            if trimmed.len() == line.len() {
                continue;
            }
            let lineno = i + 1;
            if allow_in_heredoc && cx.facts.heredocs.iter().any(|h| h.first <= lineno && lineno < h.terminator) {
                continue;
            }
            let start = cx.src.line_start(lineno);
            cx.add(start + trimmed.len(), start + line.len(), "Trailing whitespace detected.");
        }
    }
}
