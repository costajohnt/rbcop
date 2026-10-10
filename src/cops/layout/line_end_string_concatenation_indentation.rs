use crate::helpers::alignment_corrector::{self, Settings};
use crate::prelude::*;

pub const NAME: &str = "Layout/LineEndStringConcatenationIndentation";
pub static COP: LineEndStringConcatenationIndentation = LineEndStringConcatenationIndentation;
pub struct LineEndStringConcatenationIndentation;

const MSG_ALIGN: &str = "Align parts of a string concatenated with backslash.";
const MSG_INDENT: &str = "Indent the first part of a string concatenated with backslash.";

impl Cop for LineEndStringConcatenationIndentation {
    fn correctable(&self) -> bool {
        true
    }

    // Multi-line Prism StringNodes are parser `dstr`s too, but their per-line `str` children are
    // multi-line, so they never qualify.
    fn kinds(&self) -> &'static [usize] {
        &[kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let dstr = node.as_interpolated_string_node().unwrap();
        let src = cx.src;
        let single_line = |n: &Node| {
            let l = n.location();
            src.line_of(l.start_offset()) == src.line_of(l.end_offset())
        };
        // strings_concatenated_with_backslash? (a heredoc's range is its opening line)
        if is_heredoc(node) || single_line(node) {
            return;
        }
        let children: Vec<Node> = dstr.parts().iter().collect();
        if children.is_empty()
            || !children.iter().all(|c| {
                matches!(kind::of(c), kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE) && single_line(c)
            })
        {
            return;
        }
        let parent = helpers::parser_parent::parser_parent_index(cx.ancestors, cx.ancestors.len());
        let always_indented = parent.is_none_or(|i| {
            let p = &cx.ancestors[i];
            match kind::of(p) {
                kind::BLOCK_NODE | kind::LAMBDA_NODE | kind::DEF_NODE | kind::IF_NODE | kind::UNLESS_NODE => true,
                kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE => true,
                // A 2+ statement list is a parser `begin`, except directly in a plain `begin..end`.
                kind::STATEMENTS_NODE => !cx.ancestors.get(i.wrapping_sub(1)).and_then(|g| g.as_begin_node()).is_some_and(
                    |b| b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none(),
                ),
                _ => false,
            }
        });
        let col = |n: &Node| src.column(n.location().start_offset()) as i64;
        let mut offenses: Vec<(usize, &str, i64)> = Vec::new();
        let check_aligned = |offenses: &mut Vec<(usize, &str, i64)>, start: usize| {
            let mut base = col(&children[start - 1]);
            for (i, child) in children.iter().enumerate().skip(start) {
                let delta = base - col(child);
                if delta != 0 {
                    offenses.push((i, MSG_ALIGN, delta));
                }
                base = col(child);
            }
        };
        if cx.cfg.str("EnforcedStyle") == Some("aligned") && !always_indented {
            check_aligned(&mut offenses, 1);
        } else {
            let Some(second) = children.get(1) else { return };
            let width = cx
                .cfg
                .int("IndentationWidth")
                .or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width"))
                .unwrap_or(2);
            let base = match parent.map(|i| &cx.ancestors[i]) {
                Some(p) if kind::of(p) == kind::ASSOC_NODE => src.column(p.location().start_offset()) as i64,
                _ => {
                    let line = src.lines[src.line_of(children[0].location().start_offset()) - 1];
                    line.find(|c: char| !matches!(c, ' ' | '\t' | '\r' | '\n' | '\x0c' | '\x0b')).map_or(0, |b| line[..b].chars().count()) as i64
                }
            };
            let delta = base + width - col(second);
            if delta != 0 {
                offenses.push((1, MSG_INDENT, delta));
            }
            check_aligned(&mut offenses, 2);
        }
        let set = Settings::of(cx);
        for (i, msg, delta) in offenses {
            let child = &children[i];
            let (s, e) = (child.location().start_offset(), child.location().end_offset());
            cx.add_fix(s, e, msg, |c| alignment_corrector::correct(c, src, set, s, e, Some(child), delta, false));
        }
    }
}
