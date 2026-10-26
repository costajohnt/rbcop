//! RuboCop's PatternMatchingHelp mixin (1.92): whether a node is used as (part of) a pattern.

use crate::prelude::*;

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// A pattern guard (`in pat if cond`): Prism wraps the pattern in an IfNode/UnlessNode that parser
/// does not have. Returns the pattern it guards.
fn guarded_pattern<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let stmts = match node {
        Node::IfNode { .. } => node.as_if_node()?.statements(),
        Node::UnlessNode { .. } => node.as_unless_node()?.statements(),
        _ => return None,
    }?;
    stmts.body().iter().next()
}

/// `in_pattern?(node)` for the node whose Prism ancestors are `ancestors` and range is `node`.
pub fn in_pattern(ancestors: &[Node], node: (usize, usize)) -> bool {
    let mut child = node;
    for i in (0..ancestors.len()).rev() {
        let a = &ancestors[i];
        match a {
            Node::InNode { .. } => {
                let pattern = a.as_in_node().unwrap().pattern();
                return span(&guarded_pattern(&pattern).unwrap_or(pattern)) == child;
            }
            Node::MatchPredicateNode { .. } => return span(&a.as_match_predicate_node().unwrap().pattern()) == child,
            Node::MatchRequiredNode { .. } => return span(&a.as_match_required_node().unwrap().pattern()) == child,
            // PATTERN_TYPES: array_pattern(_with_tail), const_pattern, find_pattern, hash_pattern,
            // match_alt, match_as, pair, begin.
            Node::ArrayPatternNode { .. }
            | Node::FindPatternNode { .. }
            | Node::HashPatternNode { .. }
            | Node::AlternationPatternNode { .. }
            | Node::CapturePatternNode { .. }
            | Node::AssocNode { .. }
            | Node::ParenthesesNode { .. } => child = span(a),
            // A guard's IfNode (an InNode's direct child) and its statements are Prism-only.
            Node::IfNode { .. } | Node::UnlessNode { .. } if i >= 1 && ancestors[i - 1].as_in_node().is_some() => {}
            Node::StatementsNode { .. }
                if i >= 2
                    && ancestors[i - 2].as_in_node().is_some()
                    && matches!(ancestors[i - 1], Node::IfNode { .. } | Node::UnlessNode { .. }) => {}
            _ => return false,
        }
    }
    false
}
