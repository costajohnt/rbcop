use crate::prelude::*;

pub const NAME: &str = "Style/RedundantAssignment";
pub static COP: RedundantAssignment = RedundantAssignment;
pub struct RedundantAssignment;

impl Cop for RedundantAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(body) = node.as_def_node().unwrap().body() {
            check_branch(cx, &body);
        }
    }
}

/// `check_branch` on the parser node a Prism node stands for.
fn check_branch(cx: &mut Cx, node: &Node) {
    match kind::of(node) {
        kind::STATEMENTS_NODE => {
            let stmts: Vec<_> = node.as_statements_node().unwrap().body().iter().collect();
            match stmts.as_slice() {
                [only] => check_branch(cx, only),
                _ => check_begin(cx, &stmts),
            }
        }
        kind::ELSE_NODE => {
            if let Some(s) = node.as_else_node().unwrap().statements() {
                check_branch(cx, &s.as_node());
            }
        }
        kind::PARENTHESES_NODE => {
            if let Some(b) = node.as_parentheses_node().unwrap().body() {
                match b.as_statements_node() {
                    Some(s) => check_begin(cx, &s.body().iter().collect::<Vec<_>>()),
                    None => check_begin(cx, &[b]),
                }
            }
        }
        kind::BEGIN_NODE => {
            let b = node.as_begin_node().unwrap();
            if let Some(ensure) = b.ensure_clause() {
                // check_ensure_node: only the `ensure` branch
                if let Some(s) = ensure.statements() {
                    check_branch(cx, &s.as_node());
                }
            } else if b.rescue_clause().is_some() {
                // check_rescue_node: body, each resbody's body, else
                if let Some(s) = b.statements() {
                    check_branch(cx, &s.as_node());
                }
                let mut r = b.rescue_clause();
                while let Some(rescue) = r {
                    if let Some(s) = rescue.statements() {
                        check_branch(cx, &s.as_node());
                    }
                    r = rescue.subsequent();
                }
                if let Some(e) = b.else_clause() {
                    check_branch(cx, &e.as_node());
                }
            } else if b.begin_keyword_loc().is_some() {
                let stmts: Vec<_> = b.statements().map(|s| s.body().iter().collect()).unwrap_or_default();
                check_begin(cx, &stmts);
            }
        }
        kind::CASE_NODE => {
            let n = node.as_case_node().unwrap();
            for w in n.conditions().iter() {
                if let Some(s) = w.as_when_node().and_then(|w| w.statements()) {
                    check_branch(cx, &s.as_node());
                }
            }
            if let Some(e) = n.else_clause() {
                check_branch(cx, &e.as_node());
            }
        }
        kind::CASE_MATCH_NODE => {
            let n = node.as_case_match_node().unwrap();
            for w in n.conditions().iter() {
                if let Some(s) = w.as_in_node().and_then(|w| w.statements()) {
                    check_branch(cx, &s.as_node());
                }
            }
            if let Some(e) = n.else_clause() {
                check_branch(cx, &e.as_node());
            }
        }
        kind::IF_NODE => {
            let n = node.as_if_node().unwrap();
            // modifier_form? / ternary?
            if n.if_keyword_loc().is_none_or(|k| k.as_slice() == b"if" && n.end_keyword_loc().is_none()) {
                return;
            }
            if let Some(s) = n.statements() {
                check_branch(cx, &s.as_node());
            }
            if let Some(e) = n.subsequent() {
                check_branch(cx, &e);
            }
        }
        kind::UNLESS_NODE => {
            let n = node.as_unless_node().unwrap();
            if n.end_keyword_loc().is_none() {
                return;
            }
            if let Some(s) = n.statements() {
                check_branch(cx, &s.as_node());
            }
            if let Some(e) = n.else_clause() {
                check_branch(cx, &e.as_node());
            }
        }
        _ => {}
    }
}

/// check_begin_node on a `begin`/`kwbegin` with these children.
fn check_begin(cx: &mut Cx, children: &[Node]) {
    let n = children.len();
    if n >= 2
        && let Some(asgn) = children[n - 2].as_local_variable_write_node()
        && children[n - 1].as_local_variable_read_node().is_some_and(|r| r.name().as_slice() == asgn.name().as_slice())
    {
        let loc = asgn.location();
        let read = children[n - 1].location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let (first, last) = (cx.src.line_of(s), cx.src.line_of(read.start_offset()));
        let comments = cx.src.comments.iter().any(|c| (first..=last).contains(&cx.src.line_of(c.start)));
        let expression = node_text(&asgn.value()).to_string();
        let (rs, re) = (read.start_offset(), read.end_offset());
        cx.add_fix(s, e, "Redundant assignment before returning detected.", |c| {
            if !comments {
                c.replace(s, e, &expression);
                c.remove(rs, re);
            }
        });
    } else if let Some(last) = children.last() {
        check_branch(cx, last);
    }
}
