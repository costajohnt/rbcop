use crate::helpers::alignment::configured_indentation_width;
use crate::helpers::if_node;
use crate::prelude::*;

pub const NAME: &str = "Style/MultilineIfModifier";
pub static COP: MultilineIfModifier = MultilineIfModifier;
pub struct MultilineIfModifier;

#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

/// `line.sub(/^\s{n}/, indentation)` on one line from `each_line`.
fn reindent(line: &str, n: usize, indentation: &str) -> String {
    let mut chars = line.char_indices();
    let mut end = 0;
    for _ in 0..n {
        match chars.next() {
            Some((i, c)) if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\x0b') => end = i + c.len_utf8(),
            _ => return line.to_string(),
        }
    }
    format!("{indentation}{}", &line[end..])
}

/// The span whose lines decide `multiline?`: a block compares its `{`/`do` with its `}`/`end`.
fn multiline_span(body: &Node) -> (usize, usize) {
    let block = match body {
        Node::CallNode { .. } => body.as_call_node().unwrap().block(),
        Node::SuperNode { .. } => body.as_super_node().unwrap().block(),
        Node::ForwardingSuperNode { .. } => body.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        Node::LambdaNode { .. } => {
            let l = body.as_lambda_node().unwrap();
            return (l.opening_loc().start_offset(), l.closing_loc().start_offset());
        }
        _ => None,
    };
    match block.and_then(|b| b.as_block_node()) {
        Some(b) => (b.opening_loc().start_offset(), b.closing_loc().start_offset()),
        None => (body.location().start_offset(), body.location().end_offset()),
    }
}

/// One `any_str` node under a subtree (the root included), as `each_node(:any_str)` sees it.
struct Str {
    /// Heredoc: (body start, terminator span without its newline, squiggly).
    heredoc: Option<(usize, (usize, usize), bool)>,
    /// Quoted literal with `loc.begin` and `loc.end`: its range.
    quoted: Option<(usize, usize)>,
}

#[derive(Default)]
struct Strs(Vec<Str>);

impl Strs {
    fn add(&mut self, node: &Node, opening: Option<ruby_prism::Location>, closing: Option<ruby_prism::Location>, body: Option<usize>) {
        let (Some(o), Some(_)) = (opening, closing) else { return };
        if let Some((_, end, _)) = helpers::heredoc::parts(node) {
            let squiggly = o.as_slice().starts_with(b"<<~");
            self.0.push(Str { heredoc: Some((body.unwrap_or(end.0), end, squiggly)), quoted: None });
        } else {
            let l = node.location();
            self.0.push(Str { heredoc: None, quoted: Some((l.start_offset(), l.end_offset())) });
        }
    }
}

impl<'pr> ruby_prism::Visit<'pr> for Strs {
    fn visit_string_node(&mut self, n: &ruby_prism::StringNode<'pr>) {
        let body = (!n.content_loc().as_slice().is_empty()).then(|| n.content_loc().start_offset());
        self.add(&n.as_node(), n.opening_loc(), n.closing_loc(), body);
    }
    fn visit_interpolated_string_node(&mut self, n: &ruby_prism::InterpolatedStringNode<'pr>) {
        let body = n.parts().iter().next().map(|p| p.location().start_offset());
        self.add(&n.as_node(), n.opening_loc(), n.closing_loc(), body);
        ruby_prism::visit_interpolated_string_node(self, n);
    }
    fn visit_x_string_node(&mut self, n: &ruby_prism::XStringNode<'pr>) {
        let body = (!n.content_loc().as_slice().is_empty()).then(|| n.content_loc().start_offset());
        self.add(&n.as_node(), Some(n.opening_loc()), Some(n.closing_loc()), body);
    }
    fn visit_interpolated_x_string_node(&mut self, n: &ruby_prism::InterpolatedXStringNode<'pr>) {
        let body = n.parts().iter().next().map(|p| p.location().start_offset());
        self.add(&n.as_node(), Some(n.opening_loc()), Some(n.closing_loc()), body);
        ruby_prism::visit_interpolated_x_string_node(self, n);
    }
}

fn strs(node: &Node) -> Vec<Str> {
    let mut v = Strs::default();
    ruby_prism::Visit::visit(&mut v, node);
    v.0
}

/// heredoc_after (1.92): the whole lines of the heredoc bodies in `part` that come after the
/// modifier line, which move along with that part.
fn heredoc_after(src: &crate::source::Source, part: &Node, last_line: usize) -> Option<(usize, usize)> {
    let ranges: Vec<(usize, usize)> = strs(part)
        .iter()
        .filter_map(|s| s.heredoc)
        .map(|(b, end, _)| (b, end.1))
        .filter(|&(b, _)| src.line_of(b) > last_line)
        .collect();
    let s = ranges.iter().map(|r| r.0).min()?;
    let e = ranges.iter().map(|r| r.1).max()?;
    Some(helpers::whole_lines::range_by_whole_lines(src.text, s, e, true))
}

/// verbatim_lines_in (1.92): lines starting inside a string keep their indentation.
fn verbatim_lines(src: &crate::source::Source, body: &Node) -> Vec<usize> {
    let mut out = Vec::new();
    for s in strs(body) {
        if let Some((b, end, squiggly)) = s.heredoc {
            if !squiggly {
                out.extend(src.line_of(b)..=src.line_of(end.0));
            }
        } else if let Some((qs, qe)) = s.quoted {
            out.extend(src.line_of(qs) + 1..=src.line_of(qe));
        }
    }
    out
}

impl Cop for MultilineIfModifier {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        if cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= start && end <= b) {
            return;
        }
        let Some(c) = if_node::cond(node) else { return };
        let Some(body) = c.body.as_ref() else { return };
        let (bs, be) = multiline_span(body);
        if !c.modifier() || cx.src.line_of(bs) == cx.src.line_of(be) {
            return;
        }
        let offset = " ".repeat(cx.src.column(start));
        let indentation = format!("{offset}{}", " ".repeat(configured_indentation_width(cx).max(0) as usize));
        let src = cx.src;
        let body_source = format!("{offset}{}", node_text(body));
        let verbatim = verbatim_lines(src, body);
        let first_line = src.line_of(body.location().start_offset());
        let indented: String = body_source
            .split_inclusive('\n')
            .enumerate()
            .map(|(i, l)| {
                if l == "\n" || verbatim.contains(&(first_line + i)) {
                    l.to_string()
                } else {
                    reindent(l, offset.chars().count(), &indentation)
                }
            })
            .collect();
        let last_line = src.line_of(end);
        let moved: Vec<Option<(usize, usize)>> = [&c.condition, body].iter().map(|p| heredoc_after(src, p, last_line)).collect();
        let heredoc_text = |r: Option<(usize, usize)>| r.map(|(a, b)| src.text[a..b].strip_suffix('\n').unwrap_or(&src.text[a..b]).to_string());
        let condition = format!("{} {}", c.keyword, node_text(&c.condition));
        let lines = [Some(condition), heredoc_text(moved[0]), Some(indented), heredoc_text(moved[1]), Some(format!("{offset}end"))];
        let replacement = lines.into_iter().flatten().collect::<Vec<_>>().join("\n");
        let msg = format!("Favor a normal {}-statement over a modifier clause in a multiline statement.", c.keyword);
        cx.add_fix(start, end, msg, |corr| {
            for (a, b) in moved.iter().flatten() {
                corr.remove(*a, *b);
            }
            corr.replace(start, end, &replacement)
        });
        cx.state::<Ignored>().0.push((start, end));
    }
}
