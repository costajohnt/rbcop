use crate::helpers::parser_tokens::{T_STRING, translated, translated_sorted};
use crate::helpers::reparsed_equivalence::verified_by_reparse;
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantLineContinuation";
pub static COP: RedundantLineContinuation = RedundantLineContinuation;
pub struct RedundantLineContinuation;

const MSG: &str = "Redundant line continuation.";

impl Cop for RedundantLineContinuation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // processed_source.ast: nothing to do without statements.
        let stmts = node.as_program_node().unwrap().statements();
        if stmts.body().iter().next().is_none() {
            return;
        }
        let src = cx.src;
        let text = src.text;
        let (ast_start, ast_end) = (stmts.location().start_offset(), stmts.location().end_offset());
        let comments = &src.comments;
        let within_comment = |(s, e): (usize, usize)| comments.iter().any(|c| c.start < e && s < c.end);

        let sorted = translated_sorted(src);
        let string_tokens: Vec<(usize, usize)> = translated(src)
            .iter()
            .filter(|t| t.kind == T_STRING || t.kind == sys::PM_TOKEN_STRING_CONTENT)
            .map(|t| (t.start, t.end))
            .collect();

        let implicit_concatenation = |(s, e): (usize, usize)| {
            let line = src.line_of(s);
            let idx = sorted.partition_point(|t| t.end <= s);
            let before = (idx < sorted.len() && idx > 0).then(|| sorted[idx - 1]);
            let ending = before.is_some_and(|t| {
                matches!(t.kind, T_STRING | sys::PM_TOKEN_STRING_END) && src.line_of(t.end) == line
            });
            if !ending {
                return false;
            }
            let after = sorted.iter().find(|t| t.start >= e);
            after.is_some_and(|t| {
                matches!(t.kind, T_STRING | sys::PM_TOKEN_STRING_BEGIN) && src.line_of(t.start) == line + 1
            })
        };

        let mut candidates: Vec<(usize, usize)> = vec![];
        let body = &text[ast_start..ast_end];
        for (i, _) in body.match_indices("\\\n") {
            let r = (ast_start + i, ast_start + i + 2);
            if within_comment(r) || implicit_concatenation(r) {
                continue;
            }
            if string_tokens.iter().any(|&(s, e)| s < r.1 && r.0 < e) {
                continue;
            }
            // leading_dot_method_chain_with_blank_line?
            let line = src.line_of(r.0);
            let current = src.lines.get(line - 1).copied().unwrap_or("").trim_matches(ruby_strip);
            let next = src.lines.get(line).copied().unwrap_or("");
            if (current.starts_with('.') || current.starts_with("&.")) && next.trim_matches(ruby_strip).is_empty() {
                continue;
            }
            candidates.push(r);
        }

        let remove_backslash = |c: &mut Corrector, r: (usize, usize)| c.remove(r.0, r.0 + 1);
        for r in verified_by_reparse(text, src.version, node, &candidates, |r| r, remove_backslash) {
            cx.add_fix(r.0, r.1, MSG, |c| c.remove(r.0, r.0 + 1));
        }

        // inspect_end_of_ruby_code_line_continuation
        let last = src.line_of(ast_end);
        let Some(line) = src.lines.get(last - 1) else { return };
        if !line.ends_with('\\') {
            return;
        }
        let end = src.line_start(last) + line.len();
        let r = (end - 1, end);
        if within_comment(r) || verified_by_reparse(text, src.version, node, &[r], |r| r, remove_backslash).is_empty() {
            return;
        }
        cx.add_fix(r.0, r.1, MSG, |c| c.remove(r.0, r.1));
    }
}

/// Ruby `String#strip` characters (ASCII whitespace and NUL).
fn ruby_strip(c: char) -> bool {
    c.is_ascii_whitespace() || c == '\0' || c == '\x0b'
}
