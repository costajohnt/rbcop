use crate::prelude::*;

pub const NAME: &str = "Layout/TrailingEmptyLines";
pub static COP: TrailingEmptyLines = TrailingEmptyLines;
pub struct TrailingEmptyLines;

impl Cop for TrailingEmptyLines {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let text = cx.src.text;
        // Skipped for files matching /\s*__END__/ or ending in "%\n\n".
        if text.is_empty() || text.contains("__END__") || text.ends_with("%\n\n") {
            return;
        }
        let wanted: i64 = if cx.cfg.str("EnforcedStyle") == Some("final_blank_line") { 1 } else { 0 };
        // `/\s*\Z/`: Ruby `\s` includes \v, which `char::is_ascii_whitespace` does not.
        let ws_len = text.len() - text.trim_end_matches(|c: char| c.is_ascii() && helpers::parser_tokens::is_ruby_space(c as u8)).len();
        let ws = &text[text.len() - ws_len..];
        let blank = ws.matches('\n').count() as i64 - 1;
        if blank == wanted {
            return;
        }
        let begin = text.len() - ws_len + usize::from(!ws.is_empty());
        let msg = match blank {
            -1 => "Final newline missing.".to_string(),
            0 => "Trailing blank line missing.".to_string(),
            n if wanted == 0 => format!("{n} trailing blank lines detected."),
            n => format!("{n} trailing blank lines instead of {wanted} detected."),
        };
        let (from, to) = (text.len() - ws_len, text.len());
        let newlines = if wanted == 0 { "\n" } else { "\n\n" };
        cx.add_fix(begin, to, msg, |c| c.replace(from, to, newlines));
    }
}
