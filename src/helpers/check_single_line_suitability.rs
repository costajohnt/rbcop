//! RuboCop's CheckSingleLineSuitability mixin.

use crate::cop::Cx;
use crate::helpers::statement_modifier::max_line_length;
use regex::Regex;
use ruby_prism::{Node, Visit};
use std::sync::LazyLock;

static DQ_SQ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"" *\\\n[ \t\r\n\f\v]*'"#).unwrap());
static SQ_DQ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"' *\\\n[ \t\r\n\f\v]*""#).unwrap());
static SAME_DQ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"" *\\\n[ \t\r\n\f\v]*""#).unwrap());
static SAME_SQ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"' *\\\n[ \t\r\n\f\v]*'"#).unwrap());
static CHAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n[ \t\r\n\f\v]*(&?\.\w)").unwrap());
static BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t\r\n\f\v]*\\?\n[ \t\r\n\f\v]*").unwrap());

/// `to_single_line(source)`.
pub fn to_single_line(source: &str) -> String {
    let s = DQ_SQ.replace_all(source, "\" + '");
    let s = SQ_DQ.replace_all(&s, "' + \"");
    let s = SAME_DQ.replace_all(&s, "");
    let s = SAME_SQ.replace_all(&s, "");
    // `\n\s*(?=(&)?\.\w)`: keep the lookahead text.
    let s = CHAIN.replace_all(&s, "$1");
    BREAK.replace_all(&s, " ").into_owned()
}

struct Unsafe<'a> {
    src: &'a crate::source::Source<'a>,
    found: bool,
    /// Start of a block not to descend into (a parser send's own block is not its descendant).
    skip_block: Option<usize>,
}

/// `DstrNode#value` contains a newline: string parts by value, nested dstrs recursively, other
/// parts (interpolations) by source.
fn dstr_value_has_newline(parts: &ruby_prism::NodeList) -> bool {
    parts.iter().any(|p| match &p {
        Node::StringNode { .. } => p.as_string_node().unwrap().unescaped().contains(&b'\n'),
        Node::InterpolatedStringNode { .. } => dstr_value_has_newline(&p.as_interpolated_string_node().unwrap().parts()),
        _ => p.location().as_slice().contains(&b'\n'),
    })
}

impl Unsafe<'_> {
    fn multiline(&self, n: &Node) -> bool {
        let l = n.location();
        self.src.line_of(l.start_offset()) != self.src.line_of(l.end_offset())
    }
}

impl<'pr> Visit<'pr> for Unsafe<'_> {
    // Concretely typed fields skip the enter hooks.
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode<'pr>) {
        self.found = true;
        ruby_prism::visit_rescue_node(self, node);
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        if self.skip_block == Some(node.location().start_offset()) {
            return;
        }
        ruby_prism::visit_block_node(self, node);
    }
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        // Several statements are a parser `begin`.
        self.found |= node.body().iter().nth(1).is_some() && self.multiline(&node.as_node());
        ruby_prism::visit_statements_node(self, node);
    }
    fn visit_ensure_node(&mut self, node: &ruby_prism::EnsureNode<'pr>) {
        self.found = true;
        ruby_prism::visit_ensure_node(self, node);
    }
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        use crate::node_kind as kind;
        self.found |= match kind::of(&node) {
            kind::IF_NODE | kind::UNLESS_NODE | kind::CASE_NODE | kind::DEF_NODE | kind::RESCUE_NODE
            | kind::RESCUE_MODIFIER_NODE | kind::ENSURE_NODE => true,
            kind::BEGIN_NODE => node.as_begin_node().unwrap().begin_keyword_loc().is_some(),
            kind::INTERPOLATED_STRING_NODE => {
                crate::ast::is_heredoc(&node)
                    || dstr_value_has_newline(&node.as_interpolated_string_node().unwrap().parts())
            }
            kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE | kind::INTERPOLATED_SYMBOL_NODE => {
                self.multiline(&node)
            }
            _ => false,
        };
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        use crate::node_kind as kind;
        self.found |= match kind::of(&node) {
            kind::STRING_NODE => {
                crate::ast::is_heredoc(&node) || node.as_string_node().unwrap().unescaped().contains(&b'\n')
            }
            kind::SYMBOL_NODE => self.multiline(&node),
            // Parser `str` children of a regexp / xstr literal hold its source.
            kind::REGULAR_EXPRESSION_NODE => {
                node.as_regular_expression_node().unwrap().content_loc().as_slice().contains(&b'\n')
            }
            kind::X_STRING_NODE => node.as_x_string_node().unwrap().content_loc().as_slice().contains(&b'\n'),
            _ => false,
        };
    }
}

/// `suitable_as_single_line?(node)` for a node spanning `start..end`; `node` is walked for
/// `safe_to_split?` (descendants only, as `each_descendant`).
pub fn suitable_as_single_line(cx: &Cx, node: &Node, start: usize, end: usize) -> bool {
    suitable_as_single_line_skipping(cx, node, start, end, None)
}

/// `suitable_as_single_line?` with the block starting at `skip_block` left out of the walk (a
/// parser `send` whose Prism CallNode owns a block).
pub fn suitable_as_single_line_skipping(
    cx: &Cx,
    node: &Node,
    start: usize,
    end: usize,
    skip_block: Option<usize>,
) -> bool {
    let src = cx.src;
    let (first, last) = (src.line_of(start), src.line_of(end));
    if let Some(max) = max_line_length(cx) {
        let joined = src.lines[first - 1..last.min(src.lines.len())].join("\n");
        if to_single_line(&joined).chars().count() > max {
            return false;
        }
    }
    if src.comments.iter().any(|c| (first..=last).contains(&src.line_of(c.start))) {
        return false;
    }
    let mut v = Unsafe { src, found: false, skip_block };
    // ponytail: `node` itself is visited too; callers pass the block owner, never an unsafe type.
    v.visit(node);
    !v.found
}

#[cfg(test)]
mod tests {
    #[test]
    fn single_line() {
        assert_eq!(super::to_single_line("foo\n  .bar(\"a\" \\\n  \"b\")\n  &.baz"), "foo.bar(\"ab\")&.baz");
        assert_eq!(super::to_single_line("a(\n  1,\n  2\n)"), "a( 1, 2 )");
    }
}
