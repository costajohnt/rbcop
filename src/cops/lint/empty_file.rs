use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyFile";
pub static COP: EmptyFile = EmptyFile;
pub struct EmptyFile;

impl Cop for EmptyFile {
    fn on_file(&self, cx: &mut Cx) {
        let empty = cx.src.raw_text.is_empty();
        // String#blank? or comment_line? (/^\s*#/) on every line.
        let only_comments = || {
            cx.src.lines.iter().all(|l| {
                let t = l.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r', '\0']);
                t.is_empty() || t.starts_with('#')
            })
        };
        if empty || (!cx.cfg.bool("AllowComments").unwrap_or(false) && only_comments()) {
            cx.add_global("Empty file detected.");
        }
    }
}
