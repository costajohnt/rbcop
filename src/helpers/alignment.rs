//! RuboCop's Alignment mixin and what AlignmentCorrector would do (for `[Correctable]` flags).

use crate::cop::Cx;
use crate::source::Source;
use ruby_prism::{InterpolatedStringNode, InterpolatedXStringNode, Location, Node, StringNode, Visit, XStringNode};

/// `configured_indentation_width`: the cop's IndentationWidth, else Layout/IndentationWidth Width, else 2.
pub fn indentation_width(cx: &Cx) -> i64 {
    cx.cfg
        .int("IndentationWidth")
        .or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width"))
        .unwrap_or(2)
}

/// `Config#cop_enabled?`: the cop's `Enabled` value is truthy (`pending` counts).
pub fn cop_enabled(cx: &Cx, name: &str) -> bool {
    cx.config.for_cop(name).truthy("Enabled")
}

/// `config.for_enabled_cop(name)[key]` as a string.
pub fn enabled_cop_str(cx: &Cx, name: &str, key: &str) -> Option<String> {
    let m = cx.config.for_cop(name);
    if !m.truthy("Enabled") {
        return None;
    }
    m.str(key).map(str::to_string)
}

/// Unicode::DisplayWidth of one char (East Asian wide = 2, combining/zero-width = 0).
// ponytail: range table covers the common wide/zero-width blocks, not the full Unicode tables.
fn char_width(c: char) -> usize {
    match c as u32 {
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F | 0x20D0..=0x20FF => 0,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
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

/// `display_column(range)`: display width of the line up to `off`.
pub fn display_column(src: &Source, off: usize) -> usize {
    display_width(&src.text[src.line_start(src.line_of(off))..off])
}

/// `begins_its_line?(range)`: the first non-space character of the line is at `off`.
pub fn begins_its_line(src: &Source, off: usize) -> bool {
    src.text[src.line_start(src.line_of(off))..off].bytes().all(|b| matches!(b, b' ' | b'\t' | b'\x0c' | b'\x0b' | b'\r'))
}

/// Index of the first non-whitespace char in a line (`line =~ /\S/`), None for blank lines.
pub fn first_non_space(line: &str) -> Option<usize> {
    line.char_indices().find(|(_, c)| !matches!(c, ' ' | '\t' | '\x0c' | '\x0b' | '\r' | '\n')).map(|(i, _)| i)
}

/// Ruby `String#blank?` (rubocop core_ext): empty or whitespace only.
pub fn blank(line: &str) -> bool {
    line.chars().all(char::is_whitespace)
}

#[derive(Default)]
struct Taboo(Vec<(usize, usize)>);

impl Taboo {
    fn add(&mut self, opening: Option<Location>, content: Option<Location>, closing: Option<Location>) {
        let (Some(o), Some(c)) = (opening, closing) else { return };
        if o.as_slice().starts_with(b"<<") {
            let body = content.map_or(c.start_offset(), |l| l.start_offset());
            let mut end = c.end_offset();
            if c.as_slice().ends_with(b"\n") {
                end -= 1;
            }
            self.0.push((body.min(c.start_offset()), end));
        } else {
            self.0.push((o.end_offset(), c.start_offset()));
        }
    }
}

impl<'pr> Visit<'pr> for Taboo {
    fn visit_string_node(&mut self, n: &StringNode<'pr>) {
        self.add(n.opening_loc(), Some(n.content_loc()), n.closing_loc());
    }
    fn visit_interpolated_string_node(&mut self, n: &InterpolatedStringNode<'pr>) {
        let content = n.parts().iter().next().map(|p| p.location());
        self.add(n.opening_loc(), content, n.closing_loc());
        ruby_prism::visit_interpolated_string_node(self, n);
    }
    fn visit_x_string_node(&mut self, n: &XStringNode<'pr>) {
        self.add(Some(n.opening_loc()), Some(n.content_loc()), Some(n.closing_loc()));
    }
    fn visit_interpolated_x_string_node(&mut self, n: &InterpolatedXStringNode<'pr>) {
        let content = n.parts().iter().next().map(|p| p.location());
        self.add(Some(n.opening_loc()), content, Some(n.closing_loc()));
        ruby_prism::visit_interpolated_x_string_node(self, n);
    }
}

/// Whether `AlignmentCorrector.correct(corrector, ps, node_or_range, delta)` makes any edit on
/// `start..end` (spaces indentation style). `node` supplies the string ranges it must not touch.
pub fn corrector_edits(src: &Source, start: usize, end: usize, delta: i64, node: Option<&Node>) -> bool {
    if delta == 0 || src.comments.iter().any(|c| c.embdoc && c.start >= start && c.end <= end) {
        return false;
    }
    let mut taboo = Taboo::default();
    if let Some(n) = node {
        taboo.visit(n);
    }
    let text = src.text.as_bytes();
    let mut pos = start;
    for line in src.text[start..end].split_inclusive('\n') {
        let d = delta.unsigned_abs() as usize;
        let (rs, re) = if delta > 0 {
            (pos, pos)
        } else if text.get(pos) == Some(&b' ') {
            (pos, pos + d)
        } else {
            (pos.saturating_sub(d), pos)
        };
        let inside = taboo.0.iter().any(|&(ts, te)| rs >= ts && re <= te);
        if !inside {
            if delta > 0 {
                if text.get(rs) != Some(&b'\n') {
                    return true;
                }
            } else if re <= text.len() && rs < re && text[rs..re].iter().all(|&b| b == b' ' || b == b'\t') {
                return true;
            }
        }
        pos += line.len();
    }
    false
}
