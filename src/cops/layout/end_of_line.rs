use crate::prelude::*;

pub const NAME: &str = "Layout/EndOfLine";
pub static COP: EndOfLine = EndOfLine;
pub struct EndOfLine;

impl Cop for EndOfLine {
    fn on_file(&self, cx: &mut Cx) {
        // `native` follows the platform; rbcop never runs on Windows.
        let crlf = cx.cfg.str("EnforcedStyle") == Some("crlf");
        let tokens = helpers::parser_tokens::sorted_tokens(cx.src);
        let last_line = tokens.last().map_or(cx.src.lines.len(), |t| cx.src.line_of(t.start));
        for (index, line) in cx.src.raw_text.split_inclusive('\n').enumerate() {
            if index >= last_line {
                break;
            }
            let msg = if crlf {
                if line.ends_with("\r\n") || (index == last_line - 1 && !line.ends_with('\n')) {
                    continue;
                }
                "Carriage return character missing."
            } else {
                if !line.ends_with('\r') && !line.ends_with("\r\n") {
                    continue;
                }
                "Carriage return character detected."
            };
            // RuboCop: normalised line start plus the raw line's length (CRLF included).
            let start = cx.src.line_start(index + 1);
            let rest = &cx.src.text[start..];
            let end = rest.char_indices().nth(line.chars().count()).map_or(cx.src.text.len(), |(i, _)| start + i);
            cx.add(start, end, msg);
            break;
        }
    }
}
