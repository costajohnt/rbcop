//! RuboCop's DocumentationComment mixin (`documentation_comment?`), with the node-range
//! approximation of `ast_with_comments` that Style/Documentation uses.

use crate::helpers::annotation_comment::{Annotation, regex as annotation_regex};
use crate::helpers::comment_association::leading;
use crate::prelude::*;
use ruby_prism::Visit;

static INTERPRETER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^#\s*(frozen_string_literal|encoding):").unwrap());
static COMMENT_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*#").unwrap());

#[derive(Default)]
struct Ends(Vec<usize>);

impl<'pr> Visit<'pr> for Ends {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.0.push(node.location().end_offset());
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.0.push(node.location().end_offset());
    }
}

/// Every Prism node's end offset under `root`, sorted (the input `documentation_comment` expects).
pub fn node_ends(root: &Node) -> Vec<usize> {
    let mut v = Ends::default();
    v.visit(root);
    v.0.sort_unstable();
    v.0
}

/// `documentation_comment?(node)` for the node at `start`: its leading comments (those
/// `ast_with_comments` gives it on earlier lines) end on the line above it, and one of them is
/// not an annotation, interpreter directive or rubocop directive. `ends` are all node ends
/// (`node_ends`), `enclosing` the start of its nearest ancestor starting before it (0 for none).
pub fn documentation_comment(cx: &Cx, ends: &[usize], start: usize, enclosing: usize) -> bool {
    let src = cx.src;
    // Frontier: the latest end of a node finished before this one starts.
    let idx = ends.partition_point(|&e| e <= start);
    let prev = idx.checked_sub(1).map(|i| ends[i]).filter(|&e| e > 0);
    let after = prev.unwrap_or(0).max(enclosing);
    let line = src.line_of(start);
    let preceding: Vec<&Comment> = leading(src, start, after, prev.map(|e| src.line_of(e - 1)))
        .into_iter()
        .filter(|c| src.line_of(c.start) < line)
        .collect();
    let Some(last) = preceding.last() else { return false };
    if line - src.line_of(last.start) != 1 || !COMMENT_LINE.is_match(src.slice(last.start, last.end)) {
        return false;
    }
    // AnnotationComment.new sorts the keywords: a String raises (a cop error).
    let annotation = cx.config.for_cop("Style/CommentAnnotation");
    let keywords = match annotation.get("Keywords") {
        Some(crate::config::value::Value::Seq(_)) => annotation.strings("Keywords"),
        // A String, or nil (a key set to `~`).
        _ => panic!("undefined method 'sort_by' for Keywords"),
    };
    let re = annotation_regex(&keywords);
    preceding.iter().any(|c| {
        let text = src.slice(c.start, c.end);
        !Annotation::new(text, &re).annotation() && !INTERPRETER.is_match(text) && crate::directives::parse(text).is_none()
    })
}

/// The `enclosing` argument of `documentation_comment`: the start of the nearest ancestor starting
/// before `start`, whose own leading comments cannot be the node's. The Comment::Associator gives a
/// parser `begin` (parentheses, a statement list) no leading comments, so `# Doc\n(class Foo`
/// documents the class; Prism's StatementsNode and ProgramNode start where their first statement
/// does and have no parser node of their own.
pub fn enclosing_start(anc: &[Node], start: usize) -> usize {
    anc.iter()
        .rev()
        .filter(|a| !matches!(a, Node::ParenthesesNode { .. } | Node::StatementsNode { .. } | Node::ProgramNode { .. }))
        .map(|a| a.location().start_offset())
        .find(|&s| s < start)
        .unwrap_or(0)
}

/// Whether an ancestor starting where the node does (a modifier: `end if x`, `end while y`) takes
/// the preceding comments: the Comment::Associator gives leading comments to the outermost node
/// starting there, skipping only `begin` bodies. `anc` are the node's Prism ancestors.
pub fn comments_taken_by_ancestor(anc: &[Node], start: usize) -> bool {
    anc.iter()
        .rev()
        .take_while(|a| a.location().start_offset() == start)
        .any(|a| !matches!(a, Node::StatementsNode { .. } | Node::ProgramNode { .. }))
}
