use crate::cop::Heredoc;
use crate::helpers::range_help::{Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Layout/TrailingWhitespace";
pub static COP: TrailingWhitespace = TrailingWhitespace;
pub struct TrailingWhitespace;

/// Ruby's [[:blank:]]: tab plus Unicode space separators.
pub fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

impl Cop for TrailingWhitespace {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let allow_in_heredoc = cx.cfg.bool("AllowInHeredoc").unwrap_or(false);
        for (i, line) in cx.src.lines.iter().enumerate() {
            let trimmed = line.trim_end_matches(is_blank);
            if trimmed.len() == line.len() {
                continue;
            }
            let lineno = i + 1;
            let heredoc = cx.facts.heredocs.iter().find(|h| h.first <= lineno && lineno < h.terminator);
            if allow_in_heredoc && heredoc.is_some() {
                continue;
            }
            let start = cx.src.line_start(lineno);
            let (from, to) = (start + trimmed.len(), start + line.len());
            let src = cx.src;
            cx.add_fix(from, to, "Trailing whitespace detected.", |c| match heredoc {
                None => c.remove(from, to),
                Some(h) => fix_in_heredoc(c, src, h, from, to),
            });
        }
    }
}

/// TrailingWhitespace#process_line_in_heredoc.
fn fix_in_heredoc(c: &mut Corrector, src: &Source, h: &Heredoc, from: usize, to: usize) {
    let body = &src.text[src.line_start(h.first)..src.line_start(h.terminator)];
    let level = indent_level(body);
    let (b, e) = range_with_surrounding_space(src.text, from, to, Surrounding::default());
    let whitespace_only = src.text[b..e].starts_with('\n') && src.text[b..e].ends_with('\n');
    let ws = &src.text[from..to];
    if whitespace_only && ws.chars().count() <= level {
        c.remove(from, to);
    } else if !h.quoted {
        let from = if whitespace_only {
            from + ws.char_indices().nth(level).map_or(ws.len(), |(i, _)| i)
        } else {
            from
        };
        c.wrap(from, to, "#{'", "'}");
    }
}

/// Heredoc#indent_level: the shortest leading whitespace of the body's non-blank lines.
fn indent_level(body: &str) -> usize {
    body.split_inclusive('\n')
        .map(|l| &l[..l.len() - l.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b').len()])
        .filter(|ws| !ws.ends_with('\n'))
        .map(|ws| ws.chars().count())
        .min()
        .unwrap_or(0)
}
