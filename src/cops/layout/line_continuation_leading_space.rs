use crate::prelude::*;

pub const NAME: &str = "Layout/LineContinuationLeadingSpace";
pub static COP: LineContinuationLeadingSpace = LineContinuationLeadingSpace;
pub struct LineContinuationLeadingSpace;

// Ruby `\s` is ASCII-only.
static LINE_1_ENDING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"['"][ \t\r\n\x0c\x0b]*\\\n"#).unwrap());
static LINE_2_BEGINNING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\A[ \t\r\n\x0c\x0b]*['"]"#).unwrap());
static LEADING_STYLE_OFFENSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([ \t\r\n\x0c\x0b]+)(['"][ \t\r\n\x0c\x0b]*\\\n)"#).unwrap());
static TRAILING_STYLE_OFFENSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\A([ \t\r\n\x0c\x0b]*['"])([ \t\r\n\x0c\x0b]+)"#).unwrap());

impl Cop for LineContinuationLeadingSpace {
    fn correctable(&self) -> bool {
        true
    }

    // Parser `dstr`s that are plain Prism StringNodes (multi-line literals) never qualify: each
    // line's `str` child spans to the next line, so no line is a continuation.
    fn kinds(&self) -> &'static [usize] {
        &[kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let dstr = node.as_interpolated_string_node().unwrap();
        let src = cx.src;
        let loc = node.location();
        // A heredoc's source is just its opening.
        if is_heredoc(node) || !src.slice(loc.start_offset(), loc.end_offset()).contains('\\') {
            return;
        }
        let leading = cx.cfg.str("EnforcedStyle") == Some("leading");
        let first = src.line_of(loc.start_offset());
        let last = src.line_of(loc.end_offset());
        // Parser `first_line...last_line` of each multi-line child (last_line from end_pos).
        let spans: Vec<(usize, usize)> = dstr
            .parts()
            .iter()
            .map(|p| (src.line_of(p.location().start_offset()), src.line_of(p.location().end_offset())))
            .collect();
        let mut end_of_first_line = src.line_start(first);
        for line_num in first..last {
            let line_one = line_text(src.text, src.line_start(line_num));
            let line_two = line_text(src.text, src.line_start(line_num + 1));
            end_of_first_line += line_one.len();
            if !line_one.ends_with("\\\n") || spans.iter().any(|&(f, l)| f <= line_num && line_num < l) {
                continue;
            }
            if leading {
                let Some(m) = LEADING_STYLE_OFFENSE.captures(line_one) else { continue };
                let (spaces, ending) = (m.get(1).unwrap().as_str(), m.get(2).unwrap().as_str());
                let end = end_of_first_line - ending.len();
                let start = end - spaces.len();
                // RuboCop's corrector raises on nil (offense lost) when line two has no opening quote.
                let Some(b) = LINE_2_BEGINNING.find(line_two) else { continue };
                let insert = end_of_first_line + b.len();
                let spaces = spaces.to_string();
                cx.add_fix(start, end, "Move trailing spaces to the start of the next line.", |c| {
                    c.remove(start, end);
                    c.replace(insert, insert, &spaces);
                });
            } else {
                let Some(m) = TRAILING_STYLE_OFFENSE.captures(line_two) else { continue };
                let (beginning, spaces) = (m.get(1).unwrap().as_str(), m.get(2).unwrap().as_str());
                let start = end_of_first_line + beginning.len();
                let end = start + spaces.len();
                let Some(e) = LINE_1_ENDING.find(line_one) else { continue };
                let insert = end_of_first_line - e.as_str().len();
                let spaces = spaces.to_string();
                cx.add_fix(start, end, "Move leading spaces to the end of the previous line.", |c| {
                    c.remove(start, end);
                    c.replace(insert, insert, &spaces);
                });
            }
        }
    }
}

/// The line starting at `start`, newline included.
fn line_text(text: &str, start: usize) -> &str {
    let rest = &text[start.min(text.len())..];
    rest.find('\n').map_or(rest, |i| &rest[..=i])
}
