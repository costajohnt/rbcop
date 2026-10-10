use crate::helpers::if_node::parser_end;
use crate::helpers::parser_parent::{is_operator_keyword, parser_parent};
use crate::prelude::*;
use ruby_prism::StatementsNode;

pub const NAME: &str = "Style/IfWithBooleanLiteralBranches";
pub static COP: IfWithBooleanLiteralBranches = IfWithBooleanLiteralBranches;
pub struct IfWithBooleanLiteralBranches;

const COMPARISON: &[&str] = &["==", "===", "!=", "<=", ">=", ">", "<"];

/// A branch that is exactly `true` (Some(true)) or `false` (Some(false)).
fn boolean(stmts: Option<StatementsNode>) -> Option<(bool, usize)> {
    let mut it = stmts?.body().iter();
    let (Some(n), None) = (it.next(), it.next()) else { return None };
    let at = n.location().start_offset();
    match n {
        Node::TrueNode { .. } => Some((true, at)),
        Node::FalseNode { .. } => Some((false, at)),
        _ => None,
    }
}

fn is_elsif(node: &Node) -> bool {
    node.as_if_node().and_then(|i| i.if_keyword_loc()).is_some_and(|k| k.as_slice() == b"elsif")
}

fn plain_send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c))
}

fn return_boolean_value(cx: &Cx, cond: Option<Node>) -> bool {
    let Some(cond) = cond else { return false };
    match cond {
        Node::ParenthesesNode { .. } => {
            let first = cond.as_parentheses_node().unwrap().body().and_then(|b| match b.as_statements_node() {
                Some(s) => s.body().iter().next(),
                None => Some(b),
            });
            return_boolean_value(cx, first)
        }
        Node::OrNode { .. } => {
            let o = cond.as_or_node().unwrap();
            return_boolean_value(cx, Some(o.left())) && return_boolean_value(cx, Some(o.right()))
        }
        Node::AndNode { .. } => return_boolean_value(cx, Some(cond.as_and_node().unwrap().right())),
        _ => {
            let Some(call) = plain_send(&cond) else { return false };
            let name = method_name(&call);
            if helpers::allowed::allowed_method(cx.cfg, name) {
                return false;
            }
            let double_negative = name == "!"
                && call.receiver().and_then(|r| plain_send(&r)).is_some_and(|r| method_name(&r) == "!");
            COMPARISON.contains(&name) || name.ends_with('?') || double_negative
        }
    }
}

impl Cop for IfWithBooleanLiteralBranches {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let (unless, cond, body, els, keyword) = if let Some(i) = node.as_if_node() {
            let els = i.subsequent().and_then(|s| s.as_else_node()).map(|e| e.statements());
            let Some(els) = els else { return };
            (false, i.predicate(), i.statements(), els, i.if_keyword_loc())
        } else {
            let u = node.as_unless_node().unwrap();
            let Some(e) = u.else_clause() else { return };
            (true, u.predicate(), u.statements(), e.statements(), Some(u.keyword_loc()))
        };
        let (Some((if_branch, branch_at)), Some((else_branch, _))) = (boolean(body), boolean(els)) else { return };
        if if_branch == else_branch {
            return;
        }
        let cond_src = node_text(&cond);
        let cond_end = cond.location().end_offset();
        if !return_boolean_value(cx, Some(cond)) {
            return;
        }
        // `multiple_elsif?`
        if parser_parent(cx.ancestors).is_some_and(is_elsif) {
            return;
        }
        let elsif = is_elsif(node);
        let start = node.location().start_offset();
        let end = parser_end(node);
        let ((rs, re), msg) = match keyword {
            None => ((cond_end, end), "Remove redundant ternary operator with boolean literal branches.".to_string()),
            Some(_) if elsif => {
                let k = keyword.unwrap();
                ((k.start_offset(), k.end_offset()), "Use `else` instead of redundant `elsif` with boolean literal branches.".to_string())
            }
            Some(k) => {
                let msg = format!("Remove redundant `{}` with boolean literal branches.", text(&k));
                ((k.start_offset(), k.end_offset()), msg)
            }
        };
        let opposite = (!unless && !if_branch) || (unless && if_branch);
        let replacement = if opposite {
            // `require_parentheses?`
            let parens = is_operator_keyword(&cond_node_kind(node))
                || plain_send(&cond_node_kind(node)).is_some_and(|c| COMPARISON.contains(&method_name(&c)));
            if parens { format!("!({cond_src})") } else { format!("!{cond_src}") }
        } else {
            cond_src.to_string()
        };
        let indent = " ".repeat(cx.src.column(branch_at));
        cx.add_fix(rs, re, msg, |c| {
            if elsif {
                c.insert_before(start, end, "else\n");
                c.replace(start, end, &format!("{indent}{replacement}"));
            } else {
                c.replace(start, end, &replacement);
            }
        });
    }
}

fn cond_node_kind<'pr>(node: &Node<'pr>) -> Node<'pr> {
    match node.as_if_node() {
        Some(i) => i.predicate(),
        None => node.as_unless_node().unwrap().predicate(),
    }
}
