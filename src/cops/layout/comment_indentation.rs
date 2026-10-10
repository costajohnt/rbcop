use crate::prelude::*;
use helpers::alignment::{blank, configured_indentation_width, first_non_space};
use helpers::alignment_corrector::{self as ac, Settings};

pub const NAME: &str = "Layout/CommentIndentation";
pub static COP: CommentIndentation = CommentIndentation;
pub struct CommentIndentation;

static LESS_INDENTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[ \t\r\n\f\v]*(end\b|[)}\]])").unwrap());
static BARE_ACCESS_MODIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[ \t\r\n\f\v]*(private|protected|public)[ \t\r\n\f\v]*(#.*)?\z").unwrap());
static TWO_ALTERNATIVES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t\r\n\f\v]*(else|elsif|when|in|rescue|ensure)\b").unwrap());
static OWN_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[ \t\r\n\f\v]*#").unwrap());

impl Cop for CommentIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let width = configured_indentation_width(cx);
        let outdent = cx.config.for_cop("Layout/AccessModifierIndentation").str("EnforcedStyle") == Some("outdent");
        let allow_alignment = cx.cfg.bool("AllowForAlignment").unwrap_or(false);
        let own_line = |c: &Comment| src.lines.get(src.line_of(c.start) - 1).is_some_and(|l| OWN_LINE.is_match(l));
        for (ix, comment) in src.comments.iter().enumerate() {
            if !own_line(comment) {
                continue;
            }
            let line = src.line_of(comment.start);
            let next_line = src.lines.get(line..).and_then(|ls| ls.iter().find(|l| !blank(l)));
            let mut correct = next_line.map_or(0, |l| {
                let less = LESS_INDENTED.is_match(l) || (outdent && BARE_ACCESS_MODIFIER.is_match(l));
                first_non_space(l).unwrap_or(0) as i64 + if less { width } else { 0 }
            });
            let column = src.column(comment.start) as i64;
            let delta = correct - column;
            if delta == 0 {
                continue;
            }
            if next_line.is_some_and(|l| TWO_ALTERNATIVES.is_match(l)) {
                correct += width;
                if column == correct {
                    continue;
                }
            }
            if allow_alignment
                && src.comments[..ix]
                    .iter()
                    .rev()
                    .find(|c| !own_line(c))
                    .is_some_and(|c| src.column(c.start) as i64 == column)
            {
                continue;
            }
            let msg = format!("Incorrect indentation detected (column {column} instead of {correct}).");
            // autocorrect_preceding_comments: earlier own-line comments in the same column,
            // separated from this one by blank lines only, move by the same delta.
            let mut targets = vec![(comment.start, comment.end)];
            let mut below = comment;
            for above in src.comments[..ix].iter().rev() {
                let blank_between =
                    (src.line_of(above.start) + 1..src.line_of(below.start)).all(|l| blank(src.lines[l - 1]));
                if src.column(above.start) != src.column(below.start) || !own_line(above) || !blank_between {
                    break;
                }
                targets.push((above.start, above.end));
                below = above;
            }
            let set = Settings::of(cx);
            cx.add_fix(comment.start, comment.end, msg, |c| {
                for &(s, e) in targets[1..].iter().chain(&targets[..1]) {
                    ac::correct(c, src, set, s, e, None, delta, false);
                }
            });
        }
    }
}
