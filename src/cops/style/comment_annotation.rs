use crate::helpers::annotation_comment::{Annotation, regex};
use crate::prelude::*;

pub const NAME: &str = "Style/CommentAnnotation";
pub static COP: CommentAnnotation = CommentAnnotation;
pub struct CommentAnnotation;

static COMMENT_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t\r\n\f\v]*#").unwrap());

impl Cop for CommentAnnotation {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        // AnnotationComment.new sorts `keywords` for its regexp: a String raises (a cop error) at
        // the first comment that gets there.
        let mut re = None;
        let colon = cx.cfg.bool("RequireColon").unwrap_or(true);
        for (i, c) in src.comments.iter().enumerate() {
            let line = src.line_of(c.start);
            let first = i == 0 || src.line_of(src.comments[i - 1].start) < line - 1;
            let line_start = src.line_start(line);
            let line_end = src.text[line_start..].find('\n').map_or(src.text.len(), |i| line_start + i);
            let inline = !COMMENT_LINE.is_match(&src.text[line_start..line_end]);
            if !first && !inline {
                continue;
            }
            let text = src.slice(c.start, c.end);
            let re = re.get_or_insert_with(|| regex(&cx.cfg.array_bare("Keywords")));
            let a = Annotation::new(text, re);
            if !a.annotation() || a.correct(colon) {
                continue;
            }
            let keyword = a.keyword.unwrap();
            let (bs, be) = a.bounds();
            let (s, e) = (c.start + bs, c.start + be);
            let msg = match (a.note.is_some(), colon) {
                (true, true) => format!(
                    "Annotation keywords like `{keyword}` should be all upper case, followed by a colon, and a space, then a note describing the problem."
                ),
                (true, false) => format!(
                    "Annotation keywords like `{keyword}` should be all upper case, followed by a space, then a note describing the problem."
                ),
                _ => format!("Annotation comment, with keyword `{keyword}`, is missing a note."),
            };
            let has_note = a.note.is_some();
            let replacement = format!("{}{} ", keyword.to_uppercase(), if colon { ":" } else { "" });
            cx.add_fix(s, e, msg, |corr| {
                if has_note {
                    corr.replace(s, e, &replacement);
                }
            });
        }
    }
}
