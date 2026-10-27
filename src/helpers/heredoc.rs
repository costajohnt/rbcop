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

/// Prism kinds that can be heredocs (parser `on_str` / `on_dstr` / `on_xstr`).
pub const KINDS: &[usize] = &[
    crate::node_kind::STRING_NODE,
    crate::node_kind::INTERPOLATED_STRING_NODE,
    crate::node_kind::X_STRING_NODE,
    crate::node_kind::INTERPOLATED_X_STRING_NODE,
];

/// Heredoc#heredoc_type over the opening source (`<<~'EOS'` -> `<<~`).
pub fn heredoc_type(opening: &str) -> &str {
    OPENING_DELIMITER.captures(opening).and_then(|c| c.get(1)).map_or("", |m| m.as_str())
}

/// Parser's `loc.heredoc_body` as far as its text goes (Prism drops the `<<~` indentation of the
/// first line): from the first body part to the terminator line start.
pub fn body_range(node: &Node) -> Option<Span> {
    let (_, (end, _), _) = parts(node)?;
    let first = match node {
        Node::StringNode { .. } => Some(node.as_string_node()?.content_loc().start_offset()),
        Node::XStringNode { .. } => Some(node.as_x_string_node()?.content_loc().start_offset()),
        Node::InterpolatedStringNode { .. } => {
            node.as_interpolated_string_node()?.parts().iter().next().map(|p| p.location().start_offset())
        }
        _ => node.as_interpolated_x_string_node()?.parts().iter().next().map(|p| p.location().start_offset()),
    };
    Some((first.unwrap_or(end).min(end), end))
}

#[derive(Default)]
struct FirstHeredoc {
    end: Option<usize>,
}

impl FirstHeredoc {
    fn add(&mut self, node: Node) {
        if self.end.is_none()
            && let Some((_, (_, end), _)) = parts(&node)
        {
            self.end = Some(end);
        }
    }
}

impl<'pr> Visit<'pr> for FirstHeredoc {
    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        self.add(node.as_node());
    }
    fn visit_interpolated_string_node(&mut self, node: &InterpolatedStringNode<'pr>) {
        self.add(node.as_node());
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_x_string_node(&mut self, node: &XStringNode<'pr>) {
        self.add(node.as_node());
    }
    fn visit_interpolated_x_string_node(&mut self, node: &InterpolatedXStringNode<'pr>) {
        self.add(node.as_node());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

/// `node.each_node(:any_str).find(&:heredoc?)&.loc&.heredoc_end&.end_pos`: the parser terminator
/// end (newline excluded) of the first heredoc in pre-order.
pub fn first_heredoc_end(node: &Node) -> Option<usize> {
    let mut v = FirstHeredoc::default();
    v.visit(node);
    v.end
}

/// `node.each_node(:any_str).select(&:heredoc?)` (the node itself included): the opening start
/// offset (parser `loc.expression.begin_pos`, so `first_line` is its line) of every heredoc.
pub fn heredoc_starts(node: &Node) -> Vec<usize> {
    struct V(Vec<usize>);
    impl<'pr> Visit<'pr> for V {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            if crate::ast::is_heredoc(&node) {
                self.0.push(node.location().start_offset());
            }
        }
        fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
            if crate::ast::is_heredoc(&node) {
                self.0.push(node.location().start_offset());
            }
        }
    }
    let mut v = V(Vec::new());
    v.visit(node);
    v.0
}
