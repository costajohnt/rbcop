//! RuboCop's ParenthesesCorrector, planned up front so it can run inside an `add_fix` closure.

use crate::autocorrect::corrector::Corrector;
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::source::Source;
use ruby_prism::Node;

pub struct ParenFix {
    open: (usize, usize),
    close: (usize, usize),
    /// `remove_close_paren` replaces `close` with a newline (a chain after a heredoc).
    close_newline: bool,
    orphan: Option<(usize, usize)>,
    heredoc_comma: Option<(usize, usize)>,
    space_after: Option<(usize, usize)>,
}

/// `node.children.last` of the parenthesized node: its heredocs end before `)` at `close_start`
/// (`heredoc_body_before_close_paren?`).
fn heredoc_body_before_close_paren(last_child: Option<&Node>, close_start: usize) -> bool {
    last_child.is_some_and(|n| crate::helpers::rspec::heredoc_ends(n).iter().any(|&(_, e)| e <= close_start))
}

/// `text_after_close_paren`: the rest of the `)` line, left-stripped.
fn text_after_close_paren<'a>(src: &Source<'a>, close_end: usize) -> &'a str {
    let text = src.text;
    let line_end = text[close_end..].find('\n').map_or(text.len(), |i| close_end + i);
    text[close_end..line_end].trim_start_matches([' ', '\t', '\x0b', '\x0c', '\r', '\0'])
}

/// `ParenthesesCorrector.heredoc_strands_code_after_close_paren?`: a heredoc body ends before `)`
/// and code other than a chained call or a comma follows `)` on its line.
pub fn heredoc_strands_code_after_close_paren(src: &Source, last_child: Option<&Node>, close: (usize, usize)) -> bool {
    if !heredoc_body_before_close_paren(last_child, close.0) {
        return false;
    }
    let after = text_after_close_paren(src, close.1);
    !after.is_empty() && !after.starts_with('.') && !after.starts_with("&.") && !after.starts_with(',')
}

/// The node with parentheses `open`/`close`: `last_child` is `node.children.last`,
/// `last_child_is_heredoc` whether `node.child_nodes.last` is a heredoc (with its range), and
/// `question` the `?` of a ternary whose condition the node is.
pub fn plan(
    src: &Source,
    open: (usize, usize),
    close: (usize, usize),
    last_child: Option<&Node>,
    heredoc_child: Option<(usize, usize)>,
    question: Option<usize>,
) -> ParenFix {
    let text = src.text;
    let open = range_with_surrounding_space(
        text,
        open.0,
        open.1,
        Surrounding { side: Side::Right, whitespace: true, ..Default::default() },
    );
    let line_start = src.line_start(src.line_of(close.0));
    let line_end = text[close.0..].find('\n').map_or(text.len(), |i| close.0 + i);
    let line = &text[line_start..line_end];
    // heredoc_above_close_paren_swallows_chain?
    let after = text_after_close_paren(src, close.1);
    let close_newline =
        heredoc_body_before_close_paren(last_child, close.0) && (after.starts_with('.') || after.starts_with("&."));
    let last_child = last_child.map(|n| (n.location().start_offset(), n.location().end_offset()));
    // comment_above_close_paren_swallows_chain?
    let swallows = last_child.is_some_and(|(_, body_end)| {
        body_end < close.0
            && text[body_end..close.0].split_inclusive('\n').any(|seg| seg.ends_with('\n') && seg.contains('#'))
            && {
                let after = text[close.1..line_end].trim_start_matches([' ', '\t', '\x0b', '\x0c', '\r']);
                !after.is_empty() && !after.starts_with('#')
            }
    });
    let close_range = range_with_surrounding_space(
        text,
        close.0,
        close.1,
        Surrounding { side: Side::Left, newlines: close_newline || !swallows, ..Default::default() },
    );
    let trimmed = line.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
    let orphan_line = trimmed.strip_prefix(')').is_some_and(|r| {
        r.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b').starts_with(',')
    });
    let (orphan, heredoc_comma) = if orphan_line {
        let mut r = range_with_surrounding_space(
            text,
            close.0,
            close.1,
            Surrounding { side: Side::Left, newlines: true, whitespace: true, continuations: true },
        );
        if heredoc_child.is_some() {
            let (ls, le) = range_by_whole_lines(text, close.0, close.1, false);
            r.1 += comma_after_paren(&text[ls..le]);
        }
        (Some(r), heredoc_child)
    } else {
        (None, None)
    };
    let space_after = question.filter(|&q| src.column(q) == src.column(close.1)).map(|_| close);
    ParenFix { open, close: close_range, close_newline, orphan, heredoc_comma, space_after }
}

/// `line.match(/(?<=\))\s*,/)[0].size`, 0 without a match.
fn comma_after_paren(line: &str) -> usize {
    let b = line.as_bytes();
    for (i, _) in line.match_indices(')') {
        let rest = &b[i + 1..];
        let ws = rest.iter().take_while(|c| c.is_ascii_whitespace() || **c == 0x0b).count();
        if rest.get(ws) == Some(&b',') {
            return ws + 1;
        }
    }
    0
}

pub fn apply(c: &mut Corrector, f: &ParenFix) {
    c.remove(f.open.0, f.open.1);
    if f.close_newline {
        c.replace(f.close.0, f.close.1, "\n");
    } else {
        c.remove(f.close.0, f.close.1);
    }
    if let Some((s, e)) = f.orphan {
        c.remove(s, e);
    }
    if let Some((s, e)) = f.heredoc_comma {
        c.insert_after(s, e, ",");
    }
    if let Some((s, e)) = f.space_after {
        c.insert_after(s, e, " ");
    }
}
