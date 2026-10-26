use crate::prelude::*;
use helpers::select_by::plain_send;
use ruby_prism::{IfNode, Visit};

pub const NAME: &str = "Lint/LiteralAsCondition";
pub static COP: LiteralAsCondition = LiteralAsCondition;
pub struct LiteralAsCondition;

fn truthy_literal(n: &Node) -> bool {
    matches!(
        crate::helpers::literal_node::folded_kind(n),
        kind::STRING_NODE
            | kind::INTERPOLATED_STRING_NODE
            | kind::X_STRING_NODE
            | kind::INTERPOLATED_X_STRING_NODE
            | kind::INTEGER_NODE
            | kind::FLOAT_NODE
            | kind::SYMBOL_NODE
            | kind::INTERPOLATED_SYMBOL_NODE
            | kind::ARRAY_NODE
            | kind::HASH_NODE
            | kind::REGULAR_EXPRESSION_NODE
            | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
            | kind::TRUE_NODE
            | kind::RANGE_NODE
            | kind::IMAGINARY_NODE
            | kind::RATIONAL_NODE
            | kind::SOURCE_FILE_NODE
            | kind::SOURCE_LINE_NODE
    )
}

fn falsey_literal(n: &Node) -> bool {
    n.as_false_node().is_some() || n.as_nil_node().is_some()
}

fn literal(n: &Node) -> bool {
    truthy_literal(n) || falsey_literal(n)
}

/// `basic_literal?` (LiteralAsCondition's own version: arrays of basic literals count).
fn basic_literal(n: &Node) -> bool {
    if let Some(a) = n.as_array_node() {
        return primitive_array(&a);
    }
    literal(n)
        && !is_parser_dstr(n)
        && !matches!(
            kind::of(n),
            kind::X_STRING_NODE
                | kind::INTERPOLATED_X_STRING_NODE
                | kind::INTERPOLATED_SYMBOL_NODE
                | kind::HASH_NODE
                | kind::RANGE_NODE
                | kind::REGULAR_EXPRESSION_NODE
                | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
        )
}

fn primitive_array(a: &ruby_prism::ArrayNode) -> bool {
    a.elements().iter().all(|e| basic_literal(&e))
}

/// The only child of a parser `begin` (parenthesized expression), if it has exactly one.
fn single_begin_child<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    let body = n.as_parentheses_node()?.body()?;
    match body.as_statements_node() {
        Some(s) => {
            let v: Vec<Node> = s.body().iter().collect();
            if v.len() == 1 { v.into_iter().next() } else { None }
        }
        None => Some(body),
    }
}

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

fn msg(src: &str) -> String {
    format!("Literal `{src}` appeared as a condition.")
}

/// `void_value_expression?`
fn void_value(n: &Node) -> bool {
    if let Some(p) = n.as_parentheses_node() {
        let last = p.body().and_then(|b| match b.as_statements_node() {
            Some(s) => s.body().iter().last(),
            None => Some(b),
        });
        return last.is_some_and(|l| void_value(&l));
    }
    n.as_return_node().is_some() || n.as_break_node().is_some() || n.as_next_node().is_some()
}

/// `check_node` / `handle_node`: literal operands of `!`, `and`/`or` and parentheses.
fn check_node(cx: &mut Cx, n: &Node) {
    if let Some(c) = plain_send(n) {
        if c.receiver().is_some() && method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"!") {
            handle_node(cx, &c.receiver().unwrap(), false);
        }
    } else if let Some(a) = n.as_and_node() {
        handle_node(cx, &a.left(), true);
        handle_node(cx, &a.right(), true);
    } else if let Some(o) = n.as_or_node() {
        handle_node(cx, &o.left(), false);
        handle_node(cx, &o.right(), false);
    } else if let Some(child) = single_begin_child(n) {
        handle_node(cx, &child, false);
    }
}

fn handle_node(cx: &mut Cx, n: &Node, parent_is_and: bool) {
    if literal(n) {
        if !parent_is_and {
            let (s, e) = range(n);
            cx.add_with(s, e, msg(node_text(n)), false);
        }
    } else if plain_send(n).is_some() || n.as_and_node().is_some() || n.as_or_node().is_some() || n.as_parentheses_node().is_some() {
        check_node(cx, n);
    }
}

fn check_case(cx: &mut Cx, cond: &Node) {
    if cond.as_array_node().is_some_and(|a| !primitive_array(&a)) || is_parser_dstr(cond) {
        return;
    }
    handle_node(cx, cond, false);
}

/// Pattern variables (`match_var`) anywhere under a pattern.
#[derive(Default)]
struct MatchVar {
    in_pattern: usize,
    found: bool,
}

impl<'pr> Visit<'pr> for MatchVar {
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        self.in_pattern += 1;
        self.visit(&node.pattern());
        self.in_pattern -= 1;
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
        self.in_pattern += 1;
        self.visit(&node.pattern());
        self.in_pattern -= 1;
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
        self.in_pattern += 1;
        self.visit(&node.pattern());
        self.in_pattern -= 1;
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        let saved = std::mem::take(&mut self.in_pattern);
        ruby_prism::visit_multi_write_node(self, node);
        self.in_pattern = saved;
    }
    fn visit_local_variable_target_node(&mut self, _: &ruby_prism::LocalVariableTargetNode<'pr>) {
        if self.in_pattern > 0 {
            self.found = true;
        }
    }
}

/// Parser's source range of an `elsif` if-node: it stops at its last branch, not at `end`.
fn elsif_end(n: &IfNode) -> usize {
    match n.subsequent() {
        Some(sub) => match sub.as_if_node() {
            Some(i) => elsif_end(&i),
            None => {
                let e = sub.as_else_node().unwrap();
                e.statements().map_or(e.else_keyword_loc().end_offset(), |s| s.location().end_offset())
            }
        },
        None => n
            .statements()
            .map(|s| s.location().end_offset())
            .or(n.then_keyword_loc().map(|t| t.end_offset()))
            .unwrap_or(n.predicate().location().end_offset()),
    }
}

/// Parser source range of an if-like node.
fn if_range(n: &Node) -> (usize, usize) {
    match n.as_if_node() {
        Some(i) if i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif") => {
            (n.location().start_offset(), elsif_end(&i))
        }
        _ => range(n),
    }
}

/// `orphans_loop_control_keyword?`: unwrapping a run-once post-loop body would leave a
/// `break`/`next`/`redo` of it outside any loop (one not nested in a block or loop of its own).
fn orphans_loop_control_keyword(body: Option<&ruby_prism::StatementsNode>) -> bool {
    #[derive(Default)]
    struct V {
        /// Per open branch node: whether it is a block or loop of its own.
        stack: Vec<bool>,
        found: bool,
    }
    impl V {
        fn control(&mut self, node: &Node) {
            if !self.stack.contains(&true) && matches!(node, Node::BreakNode { .. } | Node::NextNode { .. } | Node::RedoNode { .. }) {
                self.found = true;
            }
        }
    }
    impl<'pr> Visit<'pr> for V {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            self.control(&node);
            let nested = match &node {
                Node::BlockNode { .. } | Node::LambdaNode { .. } | Node::WhileNode { .. } | Node::UntilNode { .. } | Node::ForNode { .. } => true,
                // parser's block node also holds the call's receiver and arguments.
                Node::CallNode { .. } => node.as_call_node().unwrap().block().is_some_and(|b| b.as_block_node().is_some()),
                _ => false,
            };
            self.stack.push(nested);
        }
        fn visit_branch_node_leave(&mut self) {
            self.stack.pop();
        }
        fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
            self.control(&node);
        }
    }
    let Some(body) = body else { return false };
    let mut v = V::default();
    v.visit_statements_node(body);
    v.found
}

#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

impl LiteralAsCondition {
    fn on_if(&self, cx: &mut Cx, node: &Node) {
        // (unless?, elsif?, ternary?, else?, condition, if_branch range, else_branch range)
        struct Parts<'pr> {
            unless: bool,
            elsif: bool,
            ternary: bool,
            has_else: bool,
            cond: Node<'pr>,
            if_branch: Option<(usize, usize)>,
            else_branch: Option<(usize, usize)>,
            else_is_elsif: bool,
        }
        let p = if let Some(i) = node.as_if_node() {
            let kw = i.if_keyword_loc();
            let ternary = kw.is_none();
            let (else_branch, else_is_elsif) = match i.subsequent() {
                Some(sub) => match sub.as_if_node() {
                    Some(e) => (Some((sub.location().start_offset(), elsif_end(&e))), true),
                    None => (sub.as_else_node().unwrap().statements().map(|s| range(&s.as_node())), false),
                },
                None => (None, false),
            };
            Parts {
                unless: false,
                elsif: kw.is_some_and(|k| k.as_slice() == b"elsif"),
                ternary,
                has_else: !ternary && i.subsequent().is_some(),
                cond: i.predicate(),
                if_branch: i.statements().map(|s| range(&s.as_node())),
                else_branch,
                else_is_elsif,
            }
        } else {
            let u = node.as_unless_node().unwrap();
            Parts {
                unless: true,
                elsif: false,
                ternary: false,
                has_else: u.else_clause().is_some(),
                cond: u.predicate(),
                if_branch: u.statements().map(|s| range(&s.as_node())),
                else_branch: u.else_clause().and_then(|e| e.statements()).map(|s| range(&s.as_node())),
                else_is_elsif: false,
            }
        };
        let cond = &p.cond;
        if !falsey_literal(cond) && !truthy_literal(cond) {
            return;
        }
        let result = if p.unless { falsey_literal(cond) } else { truthy_literal(cond) };
        let surviving = if result { p.if_branch } else { p.else_branch };
        if surviving.is_none() && (p.elsif || p.has_else) {
            return;
        }
        let src = |r: (usize, usize)| cx.src.slice(r.0, r.1);
        let new_node = if p.elsif && result {
            // range_with_comments(if_branch): plus the comments associated with it, leading ones and
            // one decorating its last line, when the branch is a single statement (a `begin`'s
            // comments go to its statements).
            let (bs, be) = p.if_branch.unwrap();
            let single = node.as_if_node().unwrap().statements().unwrap().body().iter().count() == 1;
            let cond_line = cx.src.line_of(cond.location().end_offset());
            let start = cx
                .src
                .comments
                .iter()
                .filter(|c| c.end <= bs && cx.src.line_of(c.start) > cond_line && c.start >= cond.location().end_offset())
                .map(|c| c.start)
                .min()
                .filter(|_| single)
                .unwrap_or(bs);
            let end = cx
                .src
                .comments
                .iter()
                .find(|c| c.start >= be && cx.src.line_of(c.start) == cx.src.line_of(be))
                .filter(|_| single)
                .map_or(be, |c| c.end);
            format!("else\n  {}", src((start, end)))
        } else if p.elsif && !result {
            format!("else\n  {}", src(p.else_branch.unwrap()))
        } else if let (Some(b), true) = (p.if_branch, result) {
            src(b).to_string()
        } else if p.else_is_elsif {
            format!("{}\nend", src(p.else_branch.unwrap()).replacen("elsif", "if", 1))
        } else if p.has_else || p.ternary {
            p.else_branch.map_or(String::new(), |b| src(b).to_string())
        } else if helpers::value_used::value_used(cx.ancestors, node.location().start_offset()) {
            "nil".to_string()
        } else {
            String::new()
        };
        let (ns, ne) = if_range(node);
        let ignored = cx.state::<Ignored>().0.iter().any(|&(s, e)| s <= ns && e >= ne);
        let (cs, ce) = range(cond);
        let mut ran = false;
        cx.add_fix(cs, ce, msg(node_text(cond)), |c| {
            if ignored {
                return;
            }
            c.replace(ns, ne, &new_node);
            ran = true;
        });
        if ran {
            cx.state::<Ignored>().0.push((ns, ne));
        }
    }

    fn on_loop(&self, cx: &mut Cx, node: &Node) {
        let (post, cond, stmts, is_while) = match node.as_while_node() {
            Some(w) => (w.is_begin_modifier(), w.predicate(), w.statements(), true),
            None => {
                let u = node.as_until_node().unwrap();
                (u.is_begin_modifier(), u.predicate(), u.statements(), false)
            }
        };
        let (keep, hit, kill) = if is_while {
            ("true", truthy_literal(&cond), falsey_literal(&cond))
        } else {
            ("false", falsey_literal(&cond), truthy_literal(&cond))
        };
        let cond_src = node_text(&cond);
        if cond_src == keep || !(hit || kill) {
            return;
        }
        let (cs, ce) = range(&cond);
        let (ns, ne) = range(node);
        let m = msg(cond_src);
        if !post {
            if hit {
                cx.add_fix(cs, ce, m, |c| c.replace(cs, ce, keep));
            } else {
                cx.add_fix(cs, ce, m, |c| c.remove(ns, ne));
            }
            return;
        }
        let replacement = if hit {
            node_text(node).replacen(cond_src, keep, 1)
        } else if orphans_loop_control_keyword(stmts.as_ref()) {
            cx.add_fix(cs, ce, m, |_| {});
            return;
        } else {
            // body.child_nodes.map(&:source).join("\n")
            // ponytail: a kwbegin with rescue/ensure has one `rescue` child in parser; statements used here.
            let body = stmts.and_then(|s| s.body().iter().next()).and_then(|b| b.as_begin_node());
            body.and_then(|b| b.statements())
                .map(|s| s.body().iter().map(|n| node_text(&n).to_string()).collect::<Vec<_>>().join("\n"))
                .unwrap_or_default()
        };
        cx.add_fix(cs, ce, m, |c| c.replace(ns, ne, &replacement));
    }

    fn on_case(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_node().unwrap();
        if let Some(cond) = case.predicate() {
            if !literal(&cond) {
                return;
            }
            check_case(cx, &cond);
            return;
        }
        for w in case.conditions().iter() {
            let conds: Vec<Node> = w.as_when_node().unwrap().conditions().iter().collect();
            if conds.is_empty() || !conds.iter().all(literal) {
                continue;
            }
            let (s, e) = (conds[0].location().start_offset(), conds.last().unwrap().location().end_offset());
            cx.add_with(s, e, msg(cx.src.slice(s, e)), false);
        }
    }

    fn on_case_match(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_match_node().unwrap();
        if let Some(cond) = case.predicate() {
            let mut v = MatchVar::default();
            v.visit(node);
            if v.found {
                return;
            }
            check_case(cx, &cond);
        }
    }
}

impl Cop for LiteralAsCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::AND_NODE,
            kind::OR_NODE,
            kind::IF_NODE,
            kind::UNLESS_NODE,
            kind::WHILE_NODE,
            kind::UNTIL_NODE,
            kind::CASE_NODE,
            kind::CASE_MATCH_NODE,
            kind::CALL_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match kind::of(node) {
            kind::AND_NODE | kind::OR_NODE => {
                let (lhs, rhs, hit) = match node.as_and_node() {
                    Some(a) => (a.left(), a.right(), truthy_literal(&a.left())),
                    None => {
                        let o = node.as_or_node().unwrap();
                        (o.left(), o.right(), falsey_literal(&o.left()))
                    }
                };
                if !hit {
                    return;
                }
                let void = void_value(&rhs);
                let rhs_src = node_text(&rhs);
                let (ls, le) = range(&lhs);
                let (ns, ne) = range(node);
                cx.add_fix(ls, le, msg(node_text(&lhs)), |c| {
                    if !void {
                        c.replace(ns, ne, rhs_src);
                    }
                });
            }
            kind::IF_NODE | kind::UNLESS_NODE => {
                if !is_pattern_guard(node, cx.ancestors) {
                    self.on_if(cx, node);
                }
            }
            kind::WHILE_NODE | kind::UNTIL_NODE => self.on_loop(cx, node),
            kind::CASE_NODE => self.on_case(cx, node),
            kind::CASE_MATCH_NODE => self.on_case_match(cx, node),
            _ => {
                let call = node.as_call_node().unwrap();
                if is_csend(&call) || method_name(&call) != "!" {
                    return;
                }
                let Some(cond) = call.receiver() else { return };
                if literal(&cond) {
                    let (s, e) = range(&cond);
                    cx.add_with(s, e, msg(node_text(&cond)), false);
                } else {
                    check_node(cx, &cond);
                }
            }
        }
    }
}
