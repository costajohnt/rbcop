use crate::prelude::*;

pub const NAME: &str = "Layout/ClosingHeredocIndentation";
pub static COP: ClosingHeredocIndentation = ClosingHeredocIndentation;
pub struct ClosingHeredocIndentation;

/// Text of the 1-based line holding `off`, without its newline.
fn line_at<'a>(src: &crate::source::Source<'a>, off: usize) -> &'a str {
    src.lines[src.line_of(off) - 1].trim_end_matches('\n')
}

fn indent(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b' ').count()
}

/// Ruby String#strip (ASCII whitespace and NUL).
fn strip(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\0'))
}

// Not select_by::plain_send: `n` is a parser parent, and the send inside a block is still a send.
fn is_send(n: &Node) -> bool {
    n.as_call_node().is_some_and(|c| !is_csend(&c))
}

impl Cop for ClosingHeredocIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE, kind::INTERPOLATED_STRING_NODE, kind::X_STRING_NODE, kind::INTERPOLATED_X_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (open, close) = match kind::of(node) {
            kind::STRING_NODE => {
                let s = node.as_string_node().unwrap();
                (s.opening_loc(), s.closing_loc())
            }
            kind::INTERPOLATED_STRING_NODE => {
                let s = node.as_interpolated_string_node().unwrap();
                (s.opening_loc(), s.closing_loc())
            }
            kind::X_STRING_NODE => {
                let s = node.as_x_string_node().unwrap();
                (Some(s.opening_loc()), Some(s.closing_loc()))
            }
            _ => {
                let s = node.as_interpolated_x_string_node().unwrap();
                (Some(s.opening_loc()), Some(s.closing_loc()))
            }
        };
        let (Some(open), Some(close)) = (open, close) else { return };
        let o = open.as_slice();
        if !o.starts_with(b"<<") {
            return;
        }
        // heredoc_type == SIMPLE_HEREDOC: OPENING_DELIMITER needs a delimiter character outside the
        // quotes, so `<<''` has no type (and is checked) while `<<-''`/`<<~''` read as `<<` with
        // the delimiter `-`/`~`.
        let empty_quotes = |q: &[u8]| q.len() == 2 && q[0] == q[1] && matches!(q[0], b'\'' | b'"' | b'`');
        let simple = match o.get(2) {
            Some(b'~' | b'-') => empty_quotes(&o[3..]),
            _ => !empty_quotes(&o[2..]),
        };
        if simple {
            return;
        }
        let src = cx.src;
        let opening = line_at(src, open.start_offset());
        // heredoc_end starts after a NUL, ^D or ^Z where the parser gem's lexer ends the line there
        // (#247, #543; Prism never closes a heredoc there), but its source_line is the whole line
        // (#544).
        let line_start = src.line_start(src.line_of(close.start_offset()));
        let start = src.text[line_start..close.start_offset()]
            .rfind(['\0', '\x04', '\x1a'])
            .map_or(line_start, |i| line_start + i + 1);
        let closing = line_at(src, close.start_offset());
        let line_end = line_start + closing.len();
        let end = src.text[start..line_end].find(['\0', '\x04', '\x1a']).map_or(line_end, |i| start + i);
        let (oi, ci) = (indent(opening), indent(closing));
        if oi == ci {
            return;
        }
        // `node.argument?` (parent is a `send` holding it as an argument) / `node.chained?`.
        let n = cx.ancestors.len();
        let parent = cx.ancestors.last();
        let argument = n >= 2
            && parent.is_some_and(|p| kind::of(p) == kind::ARGUMENTS_NODE)
            && is_send(&cx.ancestors[n - 2]);
        let chained = parent.and_then(|p| p.as_call_node()).is_some_and(|c| {
            c.receiver().is_some_and(|r| r.location().start_offset() == node.location().start_offset())
        });
        if argument || chained {
            // find_node_used_heredoc_argument(node.parent): climb while the parent is a `send`.
            let mut i = if argument { n - 2 } else { n - 1 };
            loop {
                let c = cx.ancestors[i].as_call_node().unwrap();
                if c.block().is_some_and(|b| kind::of(&b) == kind::BLOCK_NODE) {
                    break;
                }
                let up = if i >= 2 && kind::of(&cx.ancestors[i - 1]) == kind::ARGUMENTS_NODE {
                    i - 2
                } else if i >= 1 {
                    i - 1
                } else {
                    break;
                };
                let direct = up + 1 == i
                    && cx.ancestors[up].as_call_node().is_some_and(|p| {
                        p.receiver().is_some_and(|r| r.location().start_offset() == cx.ancestors[i].location().start_offset())
                    });
                if !is_send(&cx.ancestors[up]) || (up + 1 == i && !direct) {
                    break;
                }
                i = up;
            }
            if indent(line_at(src, cx.ancestors[i].location().start_offset())) == ci {
                return;
            }
        }
        let msg = format!(
            "`{}` is not aligned with `{}`{}.",
            strip(closing),
            strip(opening),
            if argument { " or beginning of method definition" } else { "" }
        );
        // indented_end: `closing_text.gsub(/^\s{closing_indent}/, ' ' * opening_indent)`
        let fixed = format!("{}{}", " ".repeat(oi), &closing[ci..]);
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &fixed));
    }
}
