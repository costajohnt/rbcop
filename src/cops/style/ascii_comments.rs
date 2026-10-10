use crate::prelude::*;

pub const NAME: &str = "Style/AsciiComments";
pub static COP: AsciiComments = AsciiComments;
pub struct AsciiComments;

impl Cop for AsciiComments {
    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let allowed = cx.cfg.strings("AllowedChars");
        for c in &src.comments {
            let text = src.slice(c.start, c.end);
            // `(non_ascii - allowed_non_ascii_chars).empty?`: each non-ASCII character must be listed
            if text.is_ascii() || text.chars().filter(|ch| !ch.is_ascii()).all(|ch| allowed.iter().any(|a| a.chars().eq([ch]))) {
                continue;
            }
            // first run of non-ASCII characters (`/[^[:ascii:]]+/`)
            let first = text.find(|ch: char| !ch.is_ascii()).unwrap();
            let len = text[first..].find(|ch: char| ch.is_ascii()).unwrap_or(text.len() - first);
            cx.add(c.start + first, c.start + first + len, "Use only ascii symbols in comments.");
        }
    }
}
