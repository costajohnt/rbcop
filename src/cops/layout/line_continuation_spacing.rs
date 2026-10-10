use crate::prelude::*;
use ruby_prism::{Location, Visit};

pub const NAME: &str = "Layout/LineContinuationSpacing";
pub static COP: LineContinuationSpacing = LineContinuationSpacing;
pub struct LineContinuationSpacing;

impl Cop for LineContinuationSpacing {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        if !src.raw_text.contains('\\') {
            return;
        }
        let no_space = match cx.cfg.str("EnforcedStyle") {
            Some("no_space") => true,
            Some("space") => false,
            _ => return,
        };
        let last_line = helpers::parser_tokens::translated(src).last().map_or(src.lines.len(), |t| src.line_of(t.start));
        let mut ignored: Option<Vec<(usize, usize)>> = None;
        let mut line_start = 0;
        for (index, line) in src.text.split_inclusive('\n').enumerate() {
            let start = line_start;
            line_start += line.len();
            if index >= last_line {
                break;
            }
            let body = line.strip_suffix('\n').unwrap_or(line);
            let Some(before) = body.strip_suffix('\\') else { continue };
            // Ruby `\s` (ASCII) run before the backslash.
            let ws = before.len() - before.trim_end_matches([' ', '\t', '\r', '\n', '\x0c', '\x0b']).len();
            let ok = if no_space { ws == 0 } else { ws == 1 };
            if ok {
                continue;
            }
            // RuboCop's column is `line.length - offense.length - 1` (one short on a last line without
            // a newline), in characters.
            let len = ws + 1;
            let chars = line.chars().count();
            let Some(col) = chars.checked_sub(len + 1) else { continue };
            let off_start = start + line.char_indices().nth(col).map_or(line.len(), |(i, _)| i);
            let off_end = off_start + len;
            let ranges = ignored.get_or_insert_with(|| {
                let mut v = Ignored { src: src.text, ranges: Vec::new() };
                v.visit(node);
                v.ranges.extend(src.comments.iter().map(|c| (c.start, c.end)));
                v.ranges
            });
            if ranges.iter().any(|&(s, e)| s <= off_start && off_end <= e) {
                continue;
            }
            let (msg, repl) = if no_space {
                ("Use zero spaces in front of backslash.", "\\")
            } else {
                ("Use one space in front of backslash.", " \\")
            };
            cx.add_fix(off_start, off_end, msg, |c| c.replace(off_start, off_end, repl));
        }
    }
}

/// `ignored_literal_ranges`: percent arrays, heredoc bodies, delimited strings and strings inside
/// regexps / xstrs.
struct Ignored<'a> {
    src: &'a str,
    ranges: Vec<(usize, usize)>,
}

impl Ignored<'_> {
    fn add(&mut self, l: Location) {
        self.ranges.push((l.start_offset(), l.end_offset()));
    }

    /// A literal with a `loc.begin`: its heredoc body, or its whole expression.
    fn string(&mut self, node: &Node, opening: Option<Location>, content: Option<usize>, closing: Option<Location>) {
        let Some(o) = opening else { return };
        if o.as_slice().starts_with(b"<<") {
            let Some(c) = closing else { return };
            let first = content.unwrap_or(c.start_offset()).min(c.start_offset());
            let body = self.src[..first].rfind('\n').map_or(0, |i| i + 1);
            let end = self.src[..c.start_offset()].rfind('\n').map_or(0, |i| i + 1).max(body);
            self.ranges.push((body, end));
        } else {
            self.add(node.location());
        }
    }

    fn parts(&mut self, parts: ruby_prism::NodeList) {
        for p in parts.iter() {
            if p.as_string_node().is_some() {
                self.add(p.location());
            }
        }
    }
}

impl<'pr> Visit<'pr> for Ignored<'_> {
    fn visit_string_node(&mut self, n: &ruby_prism::StringNode<'pr>) {
        let content = (!n.content_loc().as_slice().is_empty()).then(|| n.content_loc().start_offset());
        self.string(&n.as_node(), n.opening_loc(), content, n.closing_loc());
    }
    fn visit_interpolated_string_node(&mut self, n: &ruby_prism::InterpolatedStringNode<'pr>) {
        let first = n.parts().iter().next().map(|p| p.location().start_offset());
        self.string(&n.as_node(), n.opening_loc(), first, n.closing_loc());
        ruby_prism::visit_interpolated_string_node(self, n);
    }
    fn visit_x_string_node(&mut self, n: &ruby_prism::XStringNode<'pr>) {
        let content = (!n.content_loc().as_slice().is_empty()).then(|| n.content_loc().start_offset());
        if n.opening_loc().as_slice().starts_with(b"<<") {
            self.string(&n.as_node(), Some(n.opening_loc()), content, Some(n.closing_loc()));
        } else {
            self.add(n.content_loc());
        }
    }
    fn visit_interpolated_x_string_node(&mut self, n: &ruby_prism::InterpolatedXStringNode<'pr>) {
        if n.opening_loc().as_slice().starts_with(b"<<") {
            let first = n.parts().iter().next().map(|p| p.location().start_offset());
            self.string(&n.as_node(), Some(n.opening_loc()), first, Some(n.closing_loc()));
        } else {
            self.parts(n.parts());
        }
        ruby_prism::visit_interpolated_x_string_node(self, n);
    }
    fn visit_regular_expression_node(&mut self, n: &ruby_prism::RegularExpressionNode<'pr>) {
        self.add(n.content_loc());
    }
    fn visit_interpolated_regular_expression_node(&mut self, n: &ruby_prism::InterpolatedRegularExpressionNode<'pr>) {
        self.parts(n.parts());
        ruby_prism::visit_interpolated_regular_expression_node(self, n);
    }
    fn visit_array_node(&mut self, n: &ruby_prism::ArrayNode<'pr>) {
        if n.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%")) {
            self.add(n.location());
        }
        ruby_prism::visit_array_node(self, n);
    }
}
