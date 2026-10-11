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
        // Alignment#configured_indentation_width
        let width = cx.cfg.int("IndentationWidth").or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width"));
        let width = width.unwrap_or(2).max(0) as usize;
        for (i, line) in cx.src.lines.iter().enumerate() {
            // /\A\s*\t+/ (spaces style) or /\A\s* +/ (tabs style)
            let ws = line.len() - line.trim_start_matches(|c: char| c.is_ascii_whitespace()).len();
            let Some(last) = line[..ws].rfind(bad) else { continue };
            let start = cx.src.line_start(i + 1);
            let end = start + last + 1;
            if cx.facts.string_ranges.iter().any(|&(s, e)| s <= start && end <= e) {
                continue;
            }
            let ws = &line[..last + 1];
            cx.add_fix(start, end, msg, |c| {
                if ws.contains('\t') {
                    c.replace(start, end, &ws.replace('\t', &" ".repeat(width)));
                } else {
                    // gsub(/\A\s+/) { "\t" * (match.size / width) }: the range is all whitespace.
                    // A zero width raises ZeroDivisionError there (a cop error); so does `/` here.
                    c.replace(start, end, &"\t".repeat(ws.chars().count() / width));
                }
            });
        }
    }
}
