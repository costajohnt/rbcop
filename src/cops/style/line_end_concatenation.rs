use crate::prelude::*;
use helpers::parser_tokens::{translated, T_STRING};
use helpers::range_help::{range_with_surrounding_space, Side, Surrounding};

pub const NAME: &str = "Style/LineEndConcatenation";
pub static COP: LineEndConcatenation = LineEndConcatenation;
pub struct LineEndConcatenation;

impl Cop for LineEndConcatenation {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let text = src.text;
        let tokens = translated(src);
        let tx = |t: &Token| &text[t.start..t.end];
        let standard = |t: &Token| match t.kind {
            T_STRING => true,
            sys::PM_TOKEN_STRING_BEGIN | sys::PM_TOKEN_STRING_END => matches!(tx(t), "'" | "\""),
            _ => false,
        };
        for i in 0..tokens.len() {
            let (Some(pred), Some(op), Some(succ)) = (tokens.get(i), tokens.get(i + 1), tokens.get(i + 2)) else {
                continue;
            };
            if !standard(succ) || !matches!(op.kind, sys::PM_TOKEN_PLUS | sys::PM_TOKEN_LESS_LESS) || !standard(pred) {
                continue;
            }
            if src.line_of(op.start) == src.line_of(succ.start) {
                continue;
            }
            // token_after_last_string
            let mut index = i + 3;
            if succ.kind == sys::PM_TOKEN_STRING_BEGIN {
                let mut ends_to_find = 1;
                while ends_to_find > 0 {
                    // ponytail: RuboCop raises (NoMethodError) past the last token; we stop there.
                    let Some(t) = tokens.get(index) else { break };
                    match t.kind {
                        sys::PM_TOKEN_STRING_BEGIN => ends_to_find += 1,
                        sys::PM_TOKEN_STRING_END => ends_to_find -= 1,
                        _ => {}
                    }
                    index += 1;
                }
            }
            if tokens.get(index).is_some_and(|t| {
                matches!(t.kind, sys::PM_TOKEN_STAR | sys::PM_TOKEN_PERCENT | sys::PM_TOKEN_DOT | sys::PM_TOKEN_BRACKET_LEFT)
            }) {
                continue;
            }
            let (s, e) = (op.start, op.end);
            let opts = Surrounding { side: Side::Right, newlines: false, ..Default::default() };
            let (rs, mut re) = range_with_surrounding_space(text, s, e, opts);
            if text.as_bytes().get(re) == Some(&b'\\') {
                re += 1;
            }
            let msg = format!("Use `\\` instead of `{}` to concatenate multiline strings.", tx(op));
            cx.add_fix(s, e, msg, |c| c.replace(rs, re, "\\"));
        }
    }
}
