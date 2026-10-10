//! RuboCop's AlignmentCorrector (`correct` and `align_end`) as edits on a `Corrector`.

use crate::autocorrect::corrector::Corrector;
use crate::cop::Cx;
use crate::source::Source;
use ruby_prism::Node;

/// Settings AlignmentCorrector reads from the config, gathered before `add_fix`.
#[derive(Clone, Copy)]
pub struct Settings {
    /// Layout/IndentationStyle is `tabs`.
    pub tabs: bool,
    /// Layout/IndentationWidth `Width` (or 2).
    pub width: i64,
}

impl Settings {
    pub fn of(cx: &Cx) -> Self {
        Settings {
            tabs: cx.config.for_cop("Layout/IndentationStyle").str("EnforcedStyle") == Some("tabs"),
            width: cx.config.for_cop("Layout/IndentationWidth").int("Width").unwrap_or(2),
        }
    }
}

/// `AlignmentCorrector.correct(corrector, processed_source, node_or_range, column_delta,
/// tab_indentation:)` on `start..end`; `node` (None for a plain range) supplies the strings
/// whose contents are never touched.
#[allow(clippy::too_many_arguments)]
pub fn correct(
    c: &mut Corrector,
    src: &Source,
    set: Settings,
    start: usize,
    end: usize,
    node: Option<&Node>,
    delta: i64,
    tab_indentation: bool,
) {
    if src.comments.iter().any(|cm| cm.embdoc && cm.start >= start && cm.end <= end) {
        return;
    }
    let taboo = node.map_or_else(Vec::new, |n| super::alignment_check::taboo_ranges(src, n));
    let within = |r: (usize, usize)| taboo.iter().any(|&(s, e)| r.0 >= s && r.1 <= e);
    let text = src.text.as_bytes();
    let mut line_begin = start;
    let lines: Vec<usize> = src.text[start..end]
        .split_inclusive('\n')
        .map(|l| {
            let p = line_begin;
            line_begin += l.len();
            p
        })
        .collect();
    if set.tabs {
        if !tab_indentation || set.width <= 0 {
            return;
        }
        for p in lines {
            let line = src.line_of(p);
            let ls = src.line_start(line);
            let text = src.lines.get(line - 1).copied().unwrap_or("");
            let leading_len = text.len() - text.trim_start_matches([' ', '\t']).len();
            if leading_len == text.len() {
                continue;
            }
            let leading = &text[..leading_len];
            if within((ls, ls + leading_len)) {
                continue;
            }
            let visual: i64 = leading.chars().map(|ch| if ch == '\t' { set.width } else { 1 }).sum();
            let target = "\t".repeat(((visual + delta).max(0) / set.width) as usize);
            if leading != target {
                c.replace(ls, ls + leading_len, &target);
            }
        }
        return;
    }
    let d = delta.unsigned_abs() as usize;
    for p in lines {
        let range = if delta > 0 {
            (p, p)
        } else if text.get(p) == Some(&b' ') {
            (p, p + d)
        } else if p >= d {
            (p - d, p)
        } else {
            continue;
        };
        if within(range) {
            continue;
        }
        if delta > 0 {
            if text.get(p) != Some(&b'\n') {
                c.insert_before(range.0, range.1, &" ".repeat(d));
            }
        } else if range.1 <= text.len()
            && range.1 > range.0
            && text[range.0..range.1].iter().all(|&b| b == b' ' || b == b'\t')
        {
            c.remove(range.0, range.1);
        }
    }
}

/// `AlignmentCorrector.align_end(corrector, processed_source, node, align_to)`: put the `end` at
/// `end_start` in column `column`.
pub fn align_end(c: &mut Corrector, src: &Source, tabs: bool, end_start: usize, column: usize) {
    let ls = src.line_start(src.line_of(end_start));
    let indentation = if tabs { "\t".repeat(column) } else { " ".repeat(column) };
    let whitespace = &src.text[ls..end_start];
    if whitespace.trim_matches(|ch: char| ch.is_ascii_whitespace() || ch == '\0' || ch == '\x0b').is_empty() {
        c.replace(ls, end_start, &indentation);
    } else {
        c.insert_after(ls, end_start, &format!("\n{indentation}"));
    }
}
