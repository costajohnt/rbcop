use crate::prelude::*;

pub const NAME: &str = "Layout/IndentationStyle";
pub static COP: IndentationStyle = IndentationStyle;
pub struct IndentationStyle;

impl Cop for IndentationStyle {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let tabs = cx.cfg.str("EnforcedStyle") == Some("tabs");
        let (bad, msg) = if tabs { (' ', "Space detected in indentation.") } else { ('\t', "Tab detected in indentation.") };
        for (i, line) in cx.src.lines.iter().enumerate() {
            // /\A\s*\t+/ (spaces style) or /\A\s* +/ (tabs style)
            let ws = line.len() - line.trim_start_matches(|c: char| c.is_ascii_whitespace()).len();
            let Some(last) = line[..ws].rfind(bad) else { continue };
            let start = cx.src.line_start(i + 1);
            let end = start + last + 1;
            if cx.facts.string_ranges.iter().any(|&(s, e)| s <= start && end <= e) {
                continue;
            }
            cx.add(start, end, msg);
        }
    }
}
