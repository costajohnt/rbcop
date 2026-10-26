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

#[cfg(test)]
mod tests {
    use ruby_prism::{Node, Visit};

    /// `in_pattern` for each lambda literal in the source, in source order.
    #[derive(Default)]
    struct Lambdas<'pr> {
        stack: Vec<Node<'pr>>,
        found: Vec<bool>,
    }

    impl<'pr> Visit<'pr> for Lambdas<'pr> {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            if node.as_lambda_node().is_some() {
                let l = node.location();
                self.found.push(super::in_pattern(&self.stack, (l.start_offset(), l.end_offset())));
            }
            self.stack.push(node);
        }
        fn visit_branch_node_leave(&mut self) {
            self.stack.pop();
        }
    }

    fn lambdas(src: &str) -> Vec<bool> {
        let result = ruby_prism::parse(src.as_bytes());
        let mut v = Lambdas::default();
        v.visit(&result.node());
        v.found
    }

    #[test]
    fn ancestor_kinds() {
        // in_pattern, pinned expression, guard, begin, const_pattern, find_pattern, hash pattern
        // pair, the `in` body (checked against RuboCop 1.92.1's Style/SymbolProc)
        let case = "case x\nin (->(y) { y.a }) then 1\nin ^(->(y) { y.b }) then 1\nin ->(y) { y.c } if z then 1\n\
                    in Foo(->(y) { y.d }) then 1\nin [*, ->(y) { y.e }, *] then 1\nin {k: ->(y) { y.f }} then 1\n\
                    in ->(y) { y.g }\n  ->(y) { y.h }\nend\n";
        assert_eq!(lambdas(case), [true, false, true, true, true, true, true, false]);
        // match_pattern, match_pattern_p with match_alt, match_as
        assert_eq!(lambdas("x => {k: ->(y) { y.i }}"), [true]);
        assert_eq!(lambdas("x in ->(y) { y.j } | Integer"), [true]);
        assert_eq!(lambdas("x in [->(y) { y.k } => c]"), [true]);
        // not in a pattern
        assert_eq!(lambdas("foo(->(y) { y.l })"), [false]);
    }
}
