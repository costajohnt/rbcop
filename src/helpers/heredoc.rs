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
