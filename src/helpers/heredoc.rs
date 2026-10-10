//! Heredoc lookups over a subtree.

use ruby_prism::{InterpolatedStringNode, InterpolatedXStringNode, Location, Node, StringNode, Visit, XStringNode};

#[derive(Default)]
struct Ends {
    max: Option<(usize, usize)>,
}

impl Ends {
    fn add(&mut self, opening: Option<Location>, closing: Option<Location>) {
        if let (Some(o), Some(c)) = (opening, closing)
            && o.as_slice().starts_with(b"<<")
            && self.max.is_none_or(|(_, e)| c.end_offset() > e)
        {
            self.max = Some((c.start_offset(), c.end_offset()));
        }
    }
}

impl<'pr> Visit<'pr> for Ends {
    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        self.add(node.opening_loc(), node.closing_loc());
    }
    fn visit_interpolated_string_node(&mut self, node: &InterpolatedStringNode<'pr>) {
        self.add(node.opening_loc(), node.closing_loc());
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_x_string_node(&mut self, node: &XStringNode<'pr>) {
        self.add(Some(node.opening_loc()), Some(node.closing_loc()));
    }
    fn visit_interpolated_x_string_node(&mut self, node: &InterpolatedXStringNode<'pr>) {
        self.add(Some(node.opening_loc()), Some(node.closing_loc()));
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

/// `node.each_descendant(:any_str).select(&:heredoc?).map { loc.heredoc_end }.max_by(&:end_pos)`
/// (the node itself included): byte range of the heredoc terminator that ends last.
pub fn last_heredoc_end(node: &Node) -> Option<(usize, usize)> {
    let mut v = Ends::default();
    v.visit(node);
    v.max
}

/// Byte range `(start, end)`.
pub type Span = (usize, usize);

/// A heredoc literal as parser sees it: `(opening, heredoc_end, children_empty)` where `opening` is
/// `loc.expression` (`<<~X`), `heredoc_end` spans the terminator line (indentation included,
/// newline excluded) and `children_empty` is parser's `node.children.empty?` (empty body).
pub fn parts(node: &Node) -> Option<(Span, Span, bool)> {
    if !crate::ast::is_heredoc(node) {
        return None;
    }
    let (opening, closing, empty) = match node {
        Node::StringNode { .. } => {
            let n = node.as_string_node()?;
            (n.opening_loc()?, n.closing_loc()?, n.content_loc().as_slice().is_empty())
        }
        Node::InterpolatedStringNode { .. } => {
            let n = node.as_interpolated_string_node()?;
            (n.opening_loc()?, n.closing_loc()?, n.parts().is_empty())
        }
        Node::XStringNode { .. } => {
            let n = node.as_x_string_node()?;
            (n.opening_loc(), n.closing_loc(), n.content_loc().as_slice().is_empty())
        }
        _ => {
            let n = node.as_interpolated_x_string_node()?;
            (n.opening_loc(), n.closing_loc(), n.parts().is_empty())
        }
    };
    let close = closing.as_slice();
    let trim = close.iter().rev().take_while(|&&b| b == b'\n' || b == b'\r').count();
    let end = (closing.start_offset(), closing.end_offset() - trim);
    Some(((opening.start_offset(), opening.end_offset()), end, empty))
}

static OPENING_DELIMITER: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r#"(<<[~-]?)['"`]?([^'"`]+)['"`]?"#).unwrap());

/// Heredoc#delimiter_string over the opening source (`<<~'EOS'` -> `EOS`).
pub fn delimiter_string(opening: &str) -> &str {
    OPENING_DELIMITER.captures(opening).and_then(|c| c.get(2)).map_or("", |m| m.as_str())
}

struct DstrBodies<'s, 'a> {
    src: &'s crate::source::Source<'a>,
    lines: Vec<usize>,
}

impl DstrBodies<'_, '_> {
    fn add(&mut self, first: Option<usize>, closing: Option<Location>) {
        let Some(c) = closing else { return };
        let last = self.src.line_of(c.start_offset());
        let first = first.map_or(last, |f| self.src.line_of(f));
        self.lines.extend(first..last);
    }
}

impl<'pr> Visit<'pr> for DstrBodies<'_, '_> {
    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        let n = node.as_node();
        if crate::ast::is_heredoc(&n) && crate::ast::is_parser_dstr(&n) {
            self.add(Some(node.content_loc().start_offset()), node.closing_loc());
        }
    }
    fn visit_interpolated_string_node(&mut self, node: &InterpolatedStringNode<'pr>) {
        if crate::ast::is_heredoc(&node.as_node()) {
            self.add(node.parts().iter().next().map(|p| p.location().start_offset()), node.closing_loc());
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

/// `node.each_node(:dstr).select(&:heredoc?).flat_map { |n| b = n.loc.heredoc_body; (b.line...b.last_line).to_a }`:
/// body lines (terminator excluded) of the parser-`dstr` heredocs in a subtree.
pub fn dstr_body_lines(src: &crate::source::Source, node: &Node) -> Vec<usize> {
    let mut v = DstrBodies { src, lines: Vec::new() };
    v.visit(node);
    v.lines
}

/// Prism kinds that can be heredocs (parser `on_str` / `on_dstr` / `on_xstr`).
pub const KINDS: &[usize] = &[
    crate::node_kind::STRING_NODE,
    crate::node_kind::INTERPOLATED_STRING_NODE,
    crate::node_kind::X_STRING_NODE,
    crate::node_kind::INTERPOLATED_X_STRING_NODE,
];
