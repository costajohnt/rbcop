use crate::prelude::*;
use helpers::comments_help::contains_comments;
use helpers::if_node::normal_keyword;
use ruby_prism::ElseNode;

pub const NAME: &str = "Style/EmptyElse";
pub static COP: EmptyElse = EmptyElse;
pub struct EmptyElse;

/// Parser `loc.else` start of an if/unless/case: the `else` keyword, or an `elsif` keyword.
fn else_start(node: &Node) -> Option<usize> {
    if let Some(n) = node.as_if_node() {
        let sub = n.subsequent()?;
        return Some(match sub.as_else_node() {
            Some(e) => e.else_keyword_loc().start_offset(),
            None => sub.location().start_offset(),
        });
    }
    else_clause(node).map(|e| e.else_keyword_loc().start_offset())
}

fn else_clause<'pr>(node: &Node<'pr>) -> Option<ElseNode<'pr>> {
    match kind::of(node) {
        kind::IF_NODE => node.as_if_node().unwrap().subsequent()?.as_else_node(),
        kind::UNLESS_NODE => node.as_unless_node().unwrap().else_clause(),
        _ => node.as_case_node().unwrap().else_clause(),
    }
}

impl Cop for EmptyElse {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let is_case = node.as_case_node().is_some();
        if !is_case && (is_pattern_guard(node, cx.ancestors) || normal_keyword(node).is_none()) {
            return;
        }
        // comment_in_else?: from the outermost `if` of an elsif chain.
        let mut i = cx.ancestors.len();
        let mut top = node;
        while top.as_if_node().is_some_and(|n| n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif")) {
            i -= 1;
            top = &cx.ancestors[i];
        }
        let src = cx.src;
        let comment_in_else = else_start(top)
            .is_some_and(|s| contains_comments(cx, src.line_of(s), src.line_of(top.location().end_offset()) + 1));
        if cx.cfg.bool("AllowComments") == Some(true) && comment_in_else {
            return;
        }
        let Some(e) = else_clause(node) else { return };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("both");
        let body: Vec<Node> = e.statements().map(|s| s.body().iter().collect()).unwrap_or_default();
        let offense = match body.as_slice() {
            [] => style != "nil",
            [n] => style != "empty" && n.as_nil_node().is_some(),
            _ => false,
        };
        if !offense {
            return;
        }
        let missing = cx.config.for_cop("Style/MissingElse");
        let missing_style = if missing.truthy("Enabled") { missing.str("EnforcedStyle") } else { None };
        let ty = if is_case { "case" } else { "if" };
        let forbidden = missing_style.is_some_and(|s| s == ty || s == "both");
        let start = e.else_keyword_loc().start_offset();
        let end_pos = match node.as_if_node() {
            Some(n) => n.end_keyword_loc().map(|l| l.start_offset()),
            None if is_case => Some(node.as_case_node().unwrap().end_keyword_loc().start_offset()),
            None => node.as_unless_node().unwrap().end_keyword_loc().map(|l| l.start_offset()),
        };
        cx.add_fix(start, e.else_keyword_loc().end_offset(), "Redundant `else`-clause.", |c| {
            if forbidden || comment_in_else {
                return;
            }
            if let Some(end_pos) = end_pos {
                c.remove(start, end_pos);
            }
        });
    }
}
