use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyConditionalBody";
pub static COP: EmptyConditionalBody = EmptyConditionalBody;
pub struct EmptyConditionalBody;

impl Cop for EmptyConditionalBody {
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
        // (keyword, then, end, body present, predicate, else keyword, else branch present)
        let (keyword, then, end_kw, body, predicate, subsequent) = match node {
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                let Some(k) = n.if_keyword_loc() else { return }; // ternary
                (k, helpers::if_node::then_keyword(&n), n.end_keyword_loc(), n.statements().is_some(), n.predicate(), n.subsequent())
            }
            _ => {
                let n = node.as_unless_node().unwrap();
                let sub = n.else_clause().map(|e| e.as_node());
                (n.keyword_loc(), n.then_keyword_loc(), n.end_keyword_loc(), n.statements().is_some(), n.predicate(), sub)
            }
        };
        let keyword = text(&keyword);
        if body {
            return;
        }
        // `end` is parser's loc.end only for if/unless (an elsif has none).
        let end_kw = end_kw.filter(|_| keyword != "elsif");
        // loc.begin is `then`, or the `;`/newline ending the condition (on the condition's line).
        let then_line = cx.src.line_of(then.as_ref().map_or(predicate.location().end_offset(), |t| t.start_offset()));
        if end_kw.as_ref().is_some_and(|e| cx.src.line_of(e.start_offset()) == then_line) {
            return;
        }
        // loc.else: the `else` or `elsif` keyword; whether there is an else branch, and whether it is an `if`.
        let (else_kw, else_branch, else_is_if) = match &subsequent {
            None => (None, false, false),
            Some(s) => match s.as_else_node() {
                Some(e) => (Some(e.else_keyword_loc()), e.statements().is_some(), single_if(e.statements())),
                None => (s.as_if_node().unwrap().if_keyword_loc(), true, true),
            },
        };
        let start = node.location().start_offset();
        let start_line = cx.src.line_of(start);
        let end_line = match &else_kw {
            Some(k) => cx.src.line_of(k.start_offset()),
            None if keyword == "elsif" => {
                // each_ancestor(:if).find(&:if?).loc.end
                let root = cx.ancestors.iter().rev().filter_map(|a| a.as_if_node()).find(|i| {
                    i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if")
                });
                match root.and_then(|r| r.end_keyword_loc()) {
                    Some(e) => cx.src.line_of(e.start_offset()),
                    None => return, // RuboCop crashes
                }
            }
            None => {
                let paren_parent = keyword == "if" && cx.ancestors.last().is_some_and(|p| {
                    // `(if x ... end)`: the parentheses' single statement.
                    p.as_statements_node().is_some_and(|s| s.body().iter().count() == 1)
                        && cx.ancestors.len() > 1
                        && cx.ancestors[cx.ancestors.len() - 2].as_parentheses_node().is_some()
                });
                if paren_parent {
                    let p = cx.ancestors[cx.ancestors.len() - 2].as_parentheses_node().unwrap();
                    cx.src.line_of(p.closing_loc().start_offset())
                } else {
                    cx.src.line_of(end_kw.as_ref().map_or(start, |e| e.start_offset()))
                }
            }
        };
        if helpers::comments_help::allow_comments(cx, start_line, end_line) {
            return;
        }
        let range_end = match &else_kw {
            Some(k) => k.start_offset(),
            None => helpers::if_node::parser_end(node),
        };
        let msg = format!("Avoid `{keyword}` branches without a body.");
        let can_simplify = else_branch && else_kw.as_ref().is_some_and(|k| k.as_slice() == b"else");
        if !can_simplify {
            cx.add_fix(start, range_end, msg, |_| {});
            return;
        }
        let else_kw = else_kw.unwrap();
        let (else_start, else_end) = (else_kw.start_offset(), else_kw.end_offset());
        let inverse = match keyword {
            "if" => "unless",
            "unless" => "if",
            _ => "",
        };
        let replacement = format!("{inverse} {}", node_text(&predicate));
        let remove = if empty_if_branch(cx.ancestors) && !else_is_if {
            (start, else_start)
        } else {
            // deletion_range(node start..condition end): through the end of the condition's line.
            let cond_end = predicate.location().end_offset();
            let line = cx.src.line_of(cond_end);
            let next = if line < cx.src.line_count() { cx.src.line_start(line + 1) } else { cx.src.text.len() };
            (start, next)
        };
        cx.add_fix(start, range_end, msg, |c| {
            c.replace(else_start, else_end, &replacement);
            c.remove(remove.0, remove.1);
        });
    }
}

/// `empty_if_branch?`: the parser parent is not an `if`, or its if-branch is missing or an
/// `if` without a body. False without a parent (the file's only statement).
fn empty_if_branch(ancestors: &[Node]) -> bool {
    for (i, a) in ancestors.iter().enumerate().rev() {
        match a {
            Node::StatementsNode { .. } if a.as_statements_node().unwrap().body().iter().count() == 1 => {
                if i > 0 && ancestors[i - 1].as_program_node().is_some() {
                    return false;
                }
            }
            Node::ElseNode { .. } => {}
            Node::IfNode { .. } | Node::UnlessNode { .. } => {
                let statements = match a {
                    Node::IfNode { .. } => a.as_if_node().unwrap().statements(),
                    _ => a.as_unless_node().unwrap().statements(),
                };
                let Some(s) = statements else { return true };
                let body: Vec<Node> = s.body().iter().collect();
                return match body.as_slice() {
                    [only] => match only {
                        Node::IfNode { .. } => only.as_if_node().unwrap().statements().is_none(),
                        Node::UnlessNode { .. } => only.as_unless_node().unwrap().statements().is_none(),
                        _ => false,
                    },
                    _ => false,
                };
            }
            _ => return true,
        }
    }
    false
}

/// The else body is a single parser `if` node (if/unless/ternary/modifier).
fn single_if(s: Option<ruby_prism::StatementsNode>) -> bool {
    s.is_some_and(|s| {
        let body: Vec<Node> = s.body().iter().collect();
        matches!(body.as_slice(), [Node::IfNode { .. } | Node::UnlessNode { .. }])
    })
}
