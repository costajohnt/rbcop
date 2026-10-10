//! RuboCop's Alignment mixin and the parts of AlignmentCorrector that decide `[Correctable]`.

use crate::cop::Cx;
use crate::source::Source;

/// `cop_config['IndentationWidth'] || config.for_cop('Layout/IndentationWidth')['Width'] || 2`.
pub fn configured_indentation_width(cx: &Cx) -> i64 {
    cx.cfg.int("IndentationWidth").or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width")).unwrap_or(2)
}

/// Unicode::DisplayWidth of one character (East Asian Wide/Fullwidth = 2, zero-width = 0).
// ponytail: range table approximation of unicode-display_width; add full tables if corpora disagree.
fn char_width(c: char) -> usize {
    let u = c as u32;
    match u {
        0 | 0x0300..=0x036F | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x2064 | 0xFE00..=0xFE0F | 0xFEFF => 0,
        0x1100..=0x115F
        | 0x231A..=0x231B
        | 0x2329..=0x232A
        | 0x23E9..=0x23EC
        | 0x23F0
        | 0x23F3
        | 0x25FD..=0x25FE
        | 0x2614..=0x2615
        | 0x2648..=0x2653
        | 0x267F
        | 0x2693
        | 0x26A1
        | 0x26AA..=0x26AB
        | 0x26BD..=0x26BE
        | 0x26C4..=0x26C5
        | 0x26CE
        | 0x26D4
        | 0x26EA
        | 0x26F2..=0x26F3
        | 0x26F5
        | 0x26FA
        | 0x26FD
        | 0x2705
        | 0x270A..=0x270B
        | 0x2728
        | 0x274C
        | 0x274E
        | 0x2753..=0x2755
        | 0x2757
        | 0x2795..=0x2797
        | 0x27B0
        | 0x27BF
        | 0x2B1B..=0x2B1C
        | 0x2B50
        | 0x2B55
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xA960..=0xA97F
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F004
        | 0x1F0CF
        | 0x1F18E
        | 0x1F191..=0x1F19A
        | 0x1F200..=0x1F251
        | 0x1F300..=0x1F64F
        | 0x1F680..=0x1F6FF
        | 0x1F900..=0x1F9FF
        | 0x1FA70..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// `display_column(range)`: display width of the line's text before `off`.
pub fn display_column(src: &Source, off: usize) -> usize {
    src.text[src.line_start(src.line_of(off))..off].chars().map(char_width).sum()
}

/// Whether `AlignmentCorrector.correct(corrector, processed_source, node, delta)` (without tab
/// indentation) makes an edit for the node spanning `start..end` whose first line moves by `delta`.
pub fn corrects(cx: &Cx, start: usize, end: usize, delta: i64) -> bool {
    let src = cx.src;
    if src.comments.iter().any(|c| c.embdoc && c.start >= start && c.end <= end) {
        return false;
    }
    if cx.config.for_cop("Layout/IndentationStyle").str("EnforcedStyle") == Some("tabs") {
        return false;
    }
    if delta > 0 {
        return src.text.as_bytes().get(start) != Some(&b'\n');
    }
    // First line: the |delta| characters before the node must be blanks.
    let before: Vec<char> = src.text[..start].chars().rev().take(delta.unsigned_abs() as usize).collect();
    before.len() == delta.unsigned_abs() as usize && before.iter().all(|&c| c == ' ' || c == '\t')
}
