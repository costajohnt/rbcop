//! Pieces of RuboCop's CommentsHelp mixin. The caller computes `find_end_line` for its node shape.

use crate::cop::Cx;

/// `processed_source.each_comment_in_lines(start_line...end_line).any?` (1-based lines).
pub fn contains_comments(cx: &Cx, start_line: usize, end_line: usize) -> bool {
    cx.src.comments.iter().any(|c| (start_line..end_line).contains(&cx.src.line_of(c.start)))
}

/// `comments_contain_disables?`: a disabled range for this cop covers `start_line...end_line`
/// or lies inside it (Range#cover? semantics).
pub fn comments_contain_disables(cx: &Cx, start_line: usize, end_line: usize) -> bool {
    crate::runner::comment_config_read();
    let Some(ranges) = cx.facts.disabled.get(cx.cop) else { return false };
    let (s, e) = (start_line as i64, end_line as i64);
    ranges.iter().any(|&(a, b)| (e > s && a <= s && e - 1 <= b) || (s <= a && b < e))
}

/// `cop_config['AllowComments'] && contains_comments?(node) && !comments_contain_disables?(node, name)`.
pub fn allow_comments(cx: &Cx, start_line: usize, end_line: usize) -> bool {
    cx.cfg.bool("AllowComments").unwrap_or(false)
        && contains_comments(cx, start_line, end_line)
        && !comments_contain_disables(cx, start_line, end_line)
}

/// CommentsHelp#find_end_line for the visited node (`cx.ancestors` are its ancestors) when it is
/// neither an `if` nor a block: the line of its right sibling (in a statement or argument list),
/// else its parser parent's `end` line (its first line when it has no `end`), else its own last
/// line.
pub fn find_end_line(cx: &Cx, node: &ruby_prism::Node) -> usize {
    use crate::node_kind as kind;
    use ruby_prism::Node;
    let anc = cx.ancestors;
    let src = cx.src;
    let start = node.location().start_offset();
    let siblings: Vec<Node> = match anc.last() {
        Some(p) if kind::of(p) == kind::STATEMENTS_NODE => p.as_statements_node().unwrap().body().iter().collect(),
        Some(p) if kind::of(p) == kind::ARGUMENTS_NODE => {
            let mut v: Vec<Node> = p.as_arguments_node().unwrap().arguments().iter().collect();
            let owner = anc.len().checked_sub(2).and_then(|i| anc[i].as_call_node());
            v.extend(owner.and_then(|c| c.block()).filter(|b| b.as_block_argument_node().is_some()));
            v
        }
        _ => Vec::new(),
    };
    if let Some(i) = siblings.iter().position(|n| n.location().start_offset() == start)
        && let Some(next) = siblings.get(i + 1)
    {
        return src.line_of(next.location().start_offset());
    }
    let Some(i) = crate::helpers::parser_parent::parser_parent_index(anc, anc.len()) else {
        return src.line_of(node.location().end_offset());
    };
    let p = &anc[i];
    let end = match p {
        Node::ClassNode { .. } => Some(p.as_class_node().unwrap().end_keyword_loc()),
        Node::ModuleNode { .. } => Some(p.as_module_node().unwrap().end_keyword_loc()),
        Node::SingletonClassNode { .. } => Some(p.as_singleton_class_node().unwrap().end_keyword_loc()),
        Node::DefNode { .. } => p.as_def_node().unwrap().end_keyword_loc(),
        Node::BeginNode { .. } => p.as_begin_node().unwrap().end_keyword_loc(),
        Node::BlockNode { .. } => Some(p.as_block_node().unwrap().closing_loc()),
        Node::LambdaNode { .. } => Some(p.as_lambda_node().unwrap().closing_loc()),
        Node::IfNode { .. } => {
            let n = p.as_if_node().unwrap();
            n.end_keyword_loc().filter(|_| n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if"))
        }
        Node::UnlessNode { .. } => p.as_unless_node().unwrap().end_keyword_loc(),
        Node::WhileNode { .. } => p.as_while_node().unwrap().closing_loc(),
        Node::UntilNode { .. } => p.as_until_node().unwrap().closing_loc(),
        Node::CaseNode { .. } => Some(p.as_case_node().unwrap().end_keyword_loc()),
        Node::CaseMatchNode { .. } => Some(p.as_case_match_node().unwrap().end_keyword_loc()),
        Node::ForNode { .. } => Some(p.as_for_node().unwrap().end_keyword_loc()),
        Node::CallNode { .. } => p.as_call_node().unwrap().closing_loc(),
        Node::SuperNode { .. } => p.as_super_node().unwrap().rparen_loc(),
        Node::ArrayNode { .. } => p.as_array_node().unwrap().closing_loc(),
        Node::HashNode { .. } => Some(p.as_hash_node().unwrap().closing_loc()),
        Node::ParenthesesNode { .. } => Some(p.as_parentheses_node().unwrap().closing_loc()),
        Node::StatementsNode { .. } => {
            i.checked_sub(1).and_then(|k| anc[k].as_parentheses_node()).map(|n| n.closing_loc())
        }
        _ => None,
    };
    src.line_of(end.map_or(p.location().start_offset(), |l| l.start_offset()))
}

/// `comments_in_range(node)`: comments starting on lines `start_line...end_line` (1-based).
pub fn comments_in_lines(cx: &Cx, start_line: usize, end_line: usize) -> Vec<crate::source::Comment> {
    cx.src.comments.iter().filter(|c| (start_line..end_line).contains(&cx.src.line_of(c.start))).copied().collect()
}
