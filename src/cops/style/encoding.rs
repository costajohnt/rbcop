use crate::helpers::magic_comment;
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/Encoding";
pub static COP: Encoding = Encoding;
pub struct Encoding;

impl Cop for Encoding {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        if src.text.is_empty() {
            return;
        }
        for (i, line) in src.lines.iter().enumerate() {
            if line.starts_with("#!") {
                continue;
            }
            if !magic_comment::valid(line) {
                break;
            }
            if !magic_comment::encoding(line).is_some_and(|e| e.eq_ignore_ascii_case("utf-8")) {
                continue;
            }
            let start = src.line_start(i + 1);
            let end = start + line.len();
            let rest = magic_comment::without_encoding(line);
            let text = src.text;
            cx.add_fix(start, end, "Unnecessary utf-8 encoding comment.", |c| {
                if rest.trim_matches(|ch: char| ch.is_ascii_whitespace() || ch == '\0').is_empty() {
                    let side = Surrounding { side: Side::Right, ..Default::default() };
                    let (s, e) = range_with_surrounding_space(text, start, end, side);
                    c.remove(s, e);
                } else {
                    c.replace(start, end, &rest);
                }
            });
        }
    }
}
