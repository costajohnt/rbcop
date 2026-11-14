//! rubocop-sorbet's ConstantScope (`statically_scoped?`) and TLetCorrection (`replace_t_let`,
//! unwrapping `T.let(value, Type)` to `value` while keeping the value's heredoc bodies).

use crate::helpers::parser_parent::parser_parent;
use crate::prelude::*;

/// ConstantScope#statically_scoped?: the casgn whose Prism ancestors are `ancestors` sits directly
/// in a class, module, singleton class or the top-level body (a `begin` parent is looked through).
pub fn statically_scoped(ancestors: &[Node]) -> bool {
    let Some(i) = helpers::parser_parent::parser_parent_index(ancestors, ancestors.len()) else { return true };
    let mut parent = &ancestors[i];
    let is_begin = matches!(parent, Node::StatementsNode { .. } | Node::ParenthesesNode { .. });
    if is_begin {
        match parser_parent(&ancestors[..i]) {
            Some(p) => parent = p,
            None => return true,
        }
    }
    matches!(parent, Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. })
}

/// A heredoc inside the value: (parser heredoc_body range, parser heredoc_end range).
fn tail_heredocs(src: &crate::source::Source, value: &Node) -> Vec<((usize, usize), (usize, usize))> {
    struct V<'s>(&'s crate::source::Source<'s>, usize, Vec<((usize, usize), (usize, usize))>);
    impl V<'_> {
        fn add(&mut self, node: &Node) {
            if let Some((_, end, _)) = helpers::heredoc::parts(node)
                && end.1 > self.1
            {
                let first = helpers::heredoc::body_range(node).map_or(end.0, |b| b.0);
                let start = if first < end.0 { self.0.line_start(self.0.line_of(first)) } else { end.0 };
                self.2.push(((start, end.0), end));
            }
        }
    }
    impl<'pr> ruby_prism::Visit<'pr> for V<'_> {
        fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
            self.add(&node.as_node());
        }
        fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
            self.add(&node.as_node());
            ruby_prism::visit_interpolated_string_node(self, node);
        }
        fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode<'pr>) {
            self.add(&node.as_node());
        }
        fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode<'pr>) {
            self.add(&node.as_node());
            ruby_prism::visit_interpolated_x_string_node(self, node);
        }
    }
    let mut v = V(src, value.location().end_offset(), Vec::new());
    ruby_prism::Visit::visit(&mut v, value);
    v.2
}

/// TLetCorrection#replace_t_let as an edit: the range to replace and its replacement.
pub fn replace_t_let(src: &crate::source::Source, t_let: (usize, usize), value: &Node) -> (usize, usize, String) {
    let text = src.text;
    let (vs, ve) = (value.location().start_offset(), value.location().end_offset());
    let mut heredocs = tail_heredocs(src, value);
    if heredocs.is_empty() {
        return (t_let.0, t_let.1, text[vs..ve].to_string());
    }
    let end = heredocs.iter().map(|h| h.1.1).chain([t_let.1]).max().unwrap();
    // marker_line_comment: the first `#` to the end of the value's line.
    let line_end = text[ve..].find('\n').map_or(text.len(), |i| ve + i);
    let comment = text[ve..line_end].find('#').map_or(String::new(), |i| format!(" {}", &text[ve + i..line_end]));
    heredocs.sort_by_key(|h| h.0.0);
    let mut parts = vec![format!("{}{comment}", &text[vs..ve])];
    parts.extend(heredocs.iter().map(|(b, e)| format!("{}{}", &text[b.0..b.1], &text[e.0..e.1])));
    (t_let.0, end, parts.join("\n"))
}
