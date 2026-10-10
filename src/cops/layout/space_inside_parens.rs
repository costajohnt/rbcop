use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideParens";
pub static COP: SpaceInsideParens = SpaceInsideParens;
pub struct SpaceInsideParens;

const MSG: &str = "Space inside parentheses detected.";
const MSG_SPACE: &str = "No space inside parentheses detected.";

fn left(t: &Token) -> bool {
    // tLPAREN, tLPAREN2; not tLPAREN_ARG (PARENTHESIS_LEFT_PARENTHESES)
    t.kind == sys::PM_TOKEN_PARENTHESIS_LEFT
}

fn right(t: &Token) -> bool {
    t.kind == sys::PM_TOKEN_PARENTHESIS_RIGHT
}

fn comment(t: &Token) -> bool {
    matches!(t.kind, sys::PM_TOKEN_COMMENT | sys::PM_TOKEN_EMBDOC_BEGIN)
}

impl Cop for SpaceInsideParens {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let text = src.text;
        let mut tokens = helpers::parser_tokens::sorted_tokens(src);
        tokens.sort_by_key(|t| t.start);
        let same_line = |a: &Token, b: &Token| src.line_of(a.start) == src.line_of(b.start);
        let space_after = |t: &Token| text[t.end..].starts_with([' ', '\t', '\n', '\r', '\x0c', '\x0b']);
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("no_space").to_string();
        let extraneous = |cx: &mut Cx, s: usize, e: usize| cx.add_fix(s, e, MSG, |c| c.remove(s, e));
        for w in tokens.windows(2) {
            let (t1, t2) = (&w[0], &w[1]);
            let parens = left(t1) || right(t2);
            if style != "space" && style != "compact" {
                if parens && !comment(t2) && same_line(t1, t2) && space_after(t1) {
                    extraneous(cx, t1.end, t2.start);
                }
                continue;
            }
            // correct_extraneous_space_in_empty_parens
            if left(t1) && right(t2) && &text[t1.start..t2.end] != "()" {
                extraneous(cx, t1.end, t2.start);
            }
            let missing = |cx: &mut Cx| {
                let ignored = !parens
                    || &text[t1.start..t2.end] == "()"
                    || comment(t2)
                    || !same_line(t1, t2)
                    || space_after(t1);
                if ignored {
                    return;
                }
                let (s, e) = if left(t1) {
                    (t2.start, t2.start + text[t2.start..].chars().next().map_or(0, char::len_utf8))
                } else {
                    (t2.start, t2.end)
                };
                cx.add_fix(s, e, MSG_SPACE, |c| c.insert_before(s, e, " "));
            };
            if style == "space" || !(left(t1) && left(t2)) && !(right(t1) && right(t2)) {
                missing(cx);
            } else if &text[t1.end..t2.start] == " " {
                extraneous(cx, t1.end, t2.start);
            }
        }
    }
}
