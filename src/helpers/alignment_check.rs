//! Alignment mixin port from batch 6 (alignment.rs and alignment_mixin.rs are earlier ports).
//! ponytail: three ports of the same mixin, to merge in review.
//! RuboCop's Alignment mixin and the parts of AlignmentCorrector that decide whether a correction
//! edits anything (which is what makes an offense `[Correctable]`).

use crate::cop::Cx;
use crate::source::Source;
use ruby_prism::{Node, Visit};

/// Unicode::DisplayWidth.of for one char (unicode-display_width 3.2, default options).
// ponytail: common wide/zero-width blocks only; a full EastAsianWidth table if corpora need it.
fn char_width(c: char) -> usize {
    let u = c as u32;
    match u {
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F => 0,
        0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x2064 | 0xFEFF => 0,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Alignment#display_column: display width of the line up to `off`.
pub fn display_column(src: &Source, off: usize) -> usize {
    display_width(&src.text[src.line_start(src.line_of(off))..off])
}

/// Util#begins_its_line?: only (Ruby `\s`) whitespace before `off` on its line.
pub fn begins_its_line(src: &Source, off: usize) -> bool {
    src.text[src.line_start(src.line_of(off))..off].bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
}

/// `line =~ /\S/` as a character index (0 when the line is blank, where Ruby gives nil).
pub fn first_non_space(line: &str) -> usize {
    line.chars().position(|c| !matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')).unwrap_or(0)
}

/// Alignment#configured_indentation_width.
pub fn configured_indentation_width(cx: &Cx) -> i64 {
    cx.cfg
        .int("IndentationWidth")
        .or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width"))
        .unwrap_or(2)
}

#[derive(Default)]
struct Taboo(Vec<(usize, usize)>);

impl Taboo {
    fn add(&mut self, opening: Option<ruby_prism::Location>, closing: Option<ruby_prism::Location>, src: &str) {
        let (Some(o), Some(c)) = (opening, closing) else { return };
        if o.as_slice().starts_with(b"<<") {
            // heredoc_body.join(heredoc_end): body start to the terminator, without its newline.
            let mut end = c.end_offset();
            if src.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\n') {
                end -= 1;
            }
            let body = src[o.end_offset()..].find('\n').map_or(o.end_offset(), |i| o.end_offset() + i + 1);
            self.0.push((body.min(c.start_offset()), end));
        } else {
            self.0.push((o.end_offset(), c.start_offset()));
        }
    }
}

struct TabooVisit<'a> {
    taboo: Taboo,
    src: &'a str,
}

impl<'pr> Visit<'pr> for TabooVisit<'_> {
    fn visit_string_node(&mut self, n: &ruby_prism::StringNode<'pr>) {
        self.taboo.add(n.opening_loc(), n.closing_loc(), self.src);
    }
    fn visit_interpolated_string_node(&mut self, n: &ruby_prism::InterpolatedStringNode<'pr>) {
        self.taboo.add(n.opening_loc(), n.closing_loc(), self.src);
        ruby_prism::visit_interpolated_string_node(self, n);
    }
    fn visit_x_string_node(&mut self, n: &ruby_prism::XStringNode<'pr>) {
        self.taboo.add(Some(n.opening_loc()), Some(n.closing_loc()), self.src);
    }
    fn visit_interpolated_x_string_node(&mut self, n: &ruby_prism::InterpolatedXStringNode<'pr>) {
        self.taboo.add(Some(n.opening_loc()), Some(n.closing_loc()), self.src);
        ruby_prism::visit_interpolated_x_string_node(self, n);
    }
}

/// Whether `AlignmentCorrector.correct(corrector, processed_source, node_or_range, column_delta)`
/// edits anything. `node` is the Prism node whose strings are off limits (None for a plain range).
pub fn corrects(cx: &Cx, start: usize, end: usize, node: Option<&Node>, delta: i64) -> bool {
    let src = cx.src;
    if src.comments.iter().any(|c| c.embdoc && c.start >= start && c.end <= end) {
        return false;
    }
    if cx.config.for_cop("Layout/IndentationStyle").str("EnforcedStyle") == Some("tabs") {
        return false;
    }
    let taboo = node.map_or_else(Vec::new, |n| {
        let mut v = TabooVisit { taboo: Taboo::default(), src: src.text };
        v.visit(n);
        v.taboo.0
    });
    let text = src.text.as_bytes();
    let mut line_begin = start;
    for line in src.text[start..end].split_inclusive('\n') {
        let p = line_begin;
        line_begin += line.len();
        let d = delta.unsigned_abs() as usize;
        let range = if delta > 0 {
            (p, p)
        } else if text.get(p) == Some(&b' ') {
            (p, p + d)
        } else if p >= d {
            (p - d, p)
        } else {
            continue;
        };
        if taboo.iter().any(|&(s, e)| range.0 >= s && range.1 <= e) {
            continue;
        }
        if delta > 0 {
            if text.get(p) != Some(&b'\n') {
                return true;
            }
        } else if range.1 <= text.len()
            && range.1 > range.0
            && text[range.0..range.1].iter().all(|&b| b == b' ' || b == b'\t')
        {
            return true;
        }
    }
    false
}

/// One item for `check_alignment`: its range and the node AlignmentCorrector would rewrite.
pub struct Item<'pr> {
    pub start: usize,
    pub end: usize,
    pub node: Option<Node<'pr>>,
}

impl<'pr> Item<'pr> {
    pub fn of(node: Node<'pr>) -> Self {
        let loc = node.location();
        Item { start: loc.start_offset(), end: loc.end_offset(), node: Some(node) }
    }
}

/// Alignment#check_alignment: report items that begin their line at a column other than
/// `base` (default: the first item's display column). An item inside a range already reported
/// by this cop is reported without a correction.
pub fn check_alignment(cx: &mut Cx, items: &[Item], base: Option<i64>, msg: &str) {
    let Some(first) = items.first() else { return };
    let base = base.unwrap_or_else(|| display_column(cx.src, first.start) as i64);
    let mut prev_line = 0;
    for item in items {
        let line = cx.src.line_of(item.start);
        if line > prev_line && begins_its_line(cx.src, item.start) {
            let delta = base - display_column(cx.src, item.start) as i64;
            if delta != 0 {
                let nested =
                    cx.out.iter().any(|o| o.cop == cx.cop && item.start >= o.start && item.end <= o.end && !o.global);
                let correctable = !nested && corrects(cx, item.start, item.end, item.node.as_ref(), delta);
                cx.add_with(item.start, item.end, msg, correctable);
            }
        }
        prev_line = line;
    }
}
