use crate::helpers::alignment as alignment;
use crate::prelude::*;

pub const NAME: &str = "Layout/HeredocIndentation";
pub static COP: HeredocIndentation = HeredocIndentation;
pub struct HeredocIndentation;

fn ruby_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// Heredoc#indent_level: the shortest leading whitespace among lines that are not blank.
fn indent_level(s: &str) -> usize {
    s.split_inclusive('\n')
        .map(|l| &l[..l.len() - l.trim_start_matches(ruby_space).len()])
        .filter(|ws| !ws.ends_with('\n'))
        .map(|ws| ws.chars().count())
        .min()
        .unwrap_or(0)
}

/// (opening, body start, terminator start, opening range) of a heredoc node.
fn heredoc_parts(node: &Node) -> Option<(usize, usize, usize, (usize, usize))> {
    let (opening, body, closing) = match kind::of(node) {
        kind::STRING_NODE => {
            let n = node.as_string_node().unwrap();
            (n.opening_loc()?, n.content_loc().start_offset(), n.closing_loc()?)
        }
        kind::X_STRING_NODE => {
            let n = node.as_x_string_node().unwrap();
            (n.opening_loc(), n.content_loc().start_offset(), n.closing_loc())
        }
        kind::INTERPOLATED_STRING_NODE => {
            let n = node.as_interpolated_string_node().unwrap();
            let closing = n.closing_loc()?;
            let body = n.parts().iter().next().map_or(closing.start_offset(), |p| p.location().start_offset());
            (n.opening_loc()?, body, closing)
        }
        kind::INTERPOLATED_X_STRING_NODE => {
            let n = node.as_interpolated_x_string_node().unwrap();
            let closing = n.closing_loc();
            let body = n.parts().iter().next().map_or(closing.start_offset(), |p| p.location().start_offset());
            (n.opening_loc(), body, closing)
        }
        _ => return None,
    };
    let body = body.min(closing.start_offset());
    opening.as_slice().starts_with(b"<<").then_some((opening.start_offset(), body, closing.start_offset(), (opening.start_offset(), opening.end_offset())))
}

impl Cop for HeredocIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.3, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE, kind::INTERPOLATED_STRING_NODE, kind::X_STRING_NODE, kind::INTERPOLATED_X_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((opening, body_start, body_end, open_range)) = heredoc_parts(node) else { return };
        let src = cx.src;
        // Prism's `<<~` content starts after the dedented whitespace: take its whole line.
        let body_start = src.line_start(src.line_of(body_start));
        let body = src.slice(body_start, body_end);
        if body.trim_matches(|c| ruby_space(c) || c == '\0').is_empty() {
            return;
        }
        let body_indent = indent_level(body);
        let indent_type = match src.text.as_bytes().get(opening + 2) {
            Some(b'~') => "<<~",
            Some(b'-') => "<<-",
            _ => "<<",
        };
        let width = alignment::configured_indentation_width(cx);
        let base_indent = indent_level(src.lines[src.line_of(opening) - 1]) as i64;
        if indent_type == "<<~" {
            if base_indent + width == body_indent as i64 {
                return;
            }
        } else if body_indent != 0 && !self.squish(cx, node) {
            return;
        }
        if self.line_too_long(cx, body, base_indent + width - body_indent as i64) {
            return;
        }
        let msg = if indent_type == "<<~" {
            format!("Use {width} spaces for indentation in a heredoc.")
        } else {
            format!("Use {width} spaces for indentation in a heredoc by using `<<~` instead of `{indent_type}`.")
        };
        let squish = indent_type != "<<~" && self.squish(cx, node);
        // adjust_squiggly: re-indent the body and (if less indented than the opening line) the end.
        let correct_level = (base_indent + width).max(0) as usize;
        let body_new = reindent(body, body_indent, correct_level, |c| matches!(c, ' ' | '\t' | '\x0b' | '\x0c'));
        let end_line = src.line_of(body_end);
        let end_range = (body_end, src.line_start(end_line) + src.lines.get(end_line - 1).map_or(0, |l| l.len()));
        let end_text = src.slice(end_range.0, end_range.1);
        let end_level = indent_level(end_text);
        let end_new = if (end_level as i64) < base_indent {
            reindent(end_text, end_level, base_indent as usize, ruby_space)
        } else {
            end_text.to_string()
        };
        // adjust_minus: `<<-X` / `<<X` becomes `<<~X`.
        let open_text = src.slice(open_range.0, open_range.1);
        let open_new = format!("<<~{}", open_text.strip_prefix("<<-").or(open_text.strip_prefix("<<")).unwrap_or(open_text));
        cx.add_fix(body_start, body_end, msg, |c| {
            if indent_type == "<<~" || squish {
                c.replace(body_start, body_end, &body_new);
                c.replace(end_range.0, end_range.1, &end_new);
            }
            if indent_type != "<<~" {
                c.replace(open_range.0, open_range.1, &open_new);
            }
        });
    }
}

/// `str.gsub(/^#{ws}{n}/, ' ' * level)`: on each line, `n` leading `ws` characters become
/// `level` spaces (lines with fewer are left alone).
fn reindent(s: &str, n: usize, level: usize, ws: fn(char) -> bool) -> String {
    let pad = " ".repeat(level);
    s.split_inclusive('\n')
        .map(|line| {
            let lead: usize = line.chars().take(n).take_while(|&c| ws(c)).map(char::len_utf8).sum();
            if line.chars().take(n).take_while(|&c| ws(c)).count() == n {
                format!("{pad}{}", &line[lead..])
            } else {
                line.to_string()
            }
        })
        .collect()
}

impl HeredocIndentation {
    /// heredoc_squish?: ActiveSupport enabled and the parent is `(send _ {:squish :squish!})`.
    fn squish(&self, cx: &Cx, node: &Node) -> bool {
        let start = node.location().start_offset();
        cx.config.all_cops().truthy("ActiveSupportExtensionsEnabled")
            && cx.parent().and_then(|p| p.as_call_node()).is_some_and(|c| {
                !is_csend(&c)
                    && matches!(method_name(&c), "squish" | "squish!")
                    && c.arguments().is_none()
                    && c.receiver().is_some_and(|r| r.location().start_offset() == start)
            })
    }

    fn line_too_long(&self, cx: &Cx, body: &str, increase: i64) -> bool {
        if !cx.config.cop("Layout/LineLength").enabled {
            return false;
        }
        let line_length = cx.config.for_cop("Layout/LineLength");
        if line_length.truthy("AllowHeredoc") {
            return false;
        }
        let max = helpers::statement_modifier::line_length_max(&line_length);
        let longest = body.split_inclusive('\n').map(|l| l.trim_end_matches('\n').chars().count()).max().unwrap_or(0);
        (longest as i64 + increase) as f64 >= max
    }
}
