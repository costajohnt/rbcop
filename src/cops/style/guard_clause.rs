use crate::prelude::*;
use helpers::whole_lines::range_by_whole_lines;
use ruby_prism::{Location, StatementsNode, Visit};
use std::collections::HashSet;

pub const NAME: &str = "Style/GuardClause";
pub static COP: GuardClause = GuardClause;
pub struct GuardClause;

type R = (usize, usize);

fn rng(l: &Location) -> R {
    (l.start_offset(), l.end_offset())
}

/// A parser `if` node (IfNode or UnlessNode), branches normalised like rubocop-ast.
struct If<'pr> {
    node: Node<'pr>,
    pred: Node<'pr>,
    keyword: Location<'pr>,
    then_kw: Option<Location<'pr>>,
    end_kw: Option<Location<'pr>>,
    else_kw: Option<Location<'pr>>,
    if_branch: Option<StatementsNode<'pr>>,
    else_branch: Option<StatementsNode<'pr>>,
    ternary: bool,
    elsif: bool,
    elsif_conditional: bool,
    inverse: &'static str,
}

fn if_parts<'pr>(n: &Node<'pr>) -> Option<If<'pr>> {
    if let Some(i) = n.as_if_node() {
        let sub = i.subsequent();
        let else_node = sub.as_ref().and_then(|s| s.as_else_node());
        return Some(If {
            node: i.as_node(),
            pred: i.predicate(),
            keyword: i.if_keyword_loc().unwrap_or_else(|| i.predicate().location()),
            then_kw: i.then_keyword_loc().filter(|t| t.as_slice() == b"then"),
            end_kw: i.end_keyword_loc(),
            else_kw: else_node.as_ref().map(|e| e.else_keyword_loc()),
            if_branch: i.statements(),
            else_branch: else_node.and_then(|e| e.statements()),
            ternary: i.if_keyword_loc().is_none(),
            elsif: i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif"),
            elsif_conditional: sub.is_some_and(|s| s.as_if_node().is_some()),
            inverse: "unless",
        });
    }
    let u = n.as_unless_node()?;
    let else_node = u.else_clause();
    Some(If {
        node: u.as_node(),
        pred: u.predicate(),
        keyword: u.keyword_loc(),
        then_kw: u.then_keyword_loc(),
        end_kw: u.end_keyword_loc(),
        else_kw: else_node.as_ref().map(|e| e.else_keyword_loc()),
        if_branch: u.statements(),
        else_branch: else_node.and_then(|e| e.statements()),
        ternary: false,
        elsif: false,
        elsif_conditional: false,
        inverse: "if",
    })
}

impl If<'_> {
    fn has_else(&self) -> bool {
        self.else_kw.is_some() || self.elsif_conditional
    }
    fn modifier(&self) -> bool {
        !self.ternary && self.end_kw.is_none()
    }
}

fn single<'pr>(s: &Option<StatementsNode<'pr>>) -> Option<Node<'pr>> {
    let s = s.as_ref()?;
    let mut it = s.body().iter();
    let first = it.next()?;
    it.next().is_none().then_some(first)
}

/// Statements of a def / block body; None for a rescue/ensure body (parser `rescue`/`ensure`).
fn body_statements<'pr>(body: Option<Node<'pr>>) -> Option<Vec<Node<'pr>>> {
    let body = body?;
    if body.as_begin_node().is_some() {
        return None;
    }
    match body.as_statements_node() {
        Some(s) => Some(s.body().iter().collect()),
        None => Some(vec![body]),
    }
}

#[derive(Default)]
struct Lvars {
    asgn: HashSet<Vec<u8>>,
    reads: HashSet<Vec<u8>>,
}

impl<'pr> Visit<'pr> for Lvars {
    fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.asgn.insert(n.name().as_slice().to_vec());
        ruby_prism::visit_local_variable_write_node(self, n);
    }
    fn visit_local_variable_target_node(&mut self, n: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.asgn.insert(n.name().as_slice().to_vec());
    }
    fn visit_local_variable_operator_write_node(&mut self, n: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.asgn.insert(n.name().as_slice().to_vec());
        ruby_prism::visit_local_variable_operator_write_node(self, n);
    }
    fn visit_local_variable_or_write_node(&mut self, n: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.asgn.insert(n.name().as_slice().to_vec());
        ruby_prism::visit_local_variable_or_write_node(self, n);
    }
    fn visit_local_variable_and_write_node(&mut self, n: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.asgn.insert(n.name().as_slice().to_vec());
        ruby_prism::visit_local_variable_and_write_node(self, n);
    }
    fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.reads.insert(n.name().as_slice().to_vec());
    }
}

/// `assigned_lvar_used_in_if_branch?` (both lookups are `each_descendant`: the nodes themselves
/// are excluded).
fn assigned_lvar_used_in_if_branch(p: &If) -> bool {
    let Some(stmts) = &p.if_branch else { return false };
    let mut cond = Lvars::default();
    match p.pred.as_local_variable_write_node() {
        Some(w) => cond.visit(&w.value()),
        None => cond.visit(&p.pred),
    }
    if cond.asgn.is_empty() {
        return false;
    }
    let mut branch = Lvars::default();
    match single(&p.if_branch) {
        Some(n) if n.as_local_variable_read_node().is_some() => {}
        _ => branch.visit(&stmts.as_node()),
    }
    cond.asgn.iter().any(|a| branch.reads.contains(a))
}

/// `guard_clause?`: the captured raise/fail/return/break/next node, and whether it is the rhs of
/// an `and`/`or` branch.
fn guard_clause<'pr>(branch: &Option<StatementsNode<'pr>>, src: &crate::source::Source) -> Option<(Node<'pr>, bool)> {
    let b = single(branch)?;
    let (node, and_or) = match &b {
        Node::AndNode { .. } => (b.as_and_node().unwrap().right(), true),
        Node::OrNode { .. } => (b.as_or_node().unwrap().right(), true),
        _ => (single(branch)?, false),
    };
    let ok = match &node {
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            c.receiver().is_none()
                && matches!(method_name(&c), "raise" | "fail")
                && c.block().is_none_or(|b| b.as_block_argument_node().is_some())
        }
        _ => matches!(kind::of(&node), kind::RETURN_NODE | kind::BREAK_NODE | kind::NEXT_NODE),
    };
    let loc = node.location();
    (ok && src.line_of(loc.start_offset()) == src.line_of(loc.end_offset())).then_some((node, and_or))
}

/// `find_heredoc_argument`: parser `heredoc_end` of the heredoc found.
fn find_heredoc(node: Option<Node>) -> Option<R> {
    let mut node = node?;
    while let Some(p) = node.as_parentheses_node() {
        let body = p.body()?;
        node = match body.as_statements_node() {
            Some(s) => s.body().iter().next()?,
            None => body,
        };
    }
    if let Some((_, end, _)) = helpers::heredoc::parts(&node) {
        return Some(end);
    }
    let call = node.as_call_node()?;
    if call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    let mut args = call_args(&call);
    if let Some(b) = call.block() {
        args.push(b);
    }
    for a in args.into_iter().rev() {
        if let Some(h) = find_heredoc(Some(a)) {
            return Some(h);
        }
    }
    find_heredoc(call.receiver())
}

/// The branch as parser's `node.children.first while begin_type?` sees it.
fn branch_first<'pr>(s: &Option<StatementsNode<'pr>>) -> Option<Node<'pr>> {
    s.as_ref()?.body().iter().next()
}

impl GuardClause {
    fn accepted_form(cx: &Cx, p: &If, ending: bool, parent_assignment: bool) -> bool {
        if p.modifier() || p.ternary || p.elsif_conditional || assigned_lvar_used_in_if_branch(p) {
            return true;
        }
        let accepted_if = if ending { p.has_else() } else { !p.has_else() || p.elsif };
        let l = p.pred.location();
        accepted_if || cx.src.line_of(l.start_offset()) != cx.src.line_of(l.end_offset()) || parent_assignment
    }

    fn check_ending_body(cx: &mut Cx, stmts: &[Node], parent_is_if_with_cond: Option<&Node>) {
        let Some(last) = stmts.last() else { return };
        let Some(p) = if_parts(last) else { return };
        if Self::accepted_form(cx, &p, true, false) {
            return;
        }
        let min = cx.cfg.int("MinBodyLength").unwrap_or(1) as usize;
        let end = p.end_kw.as_ref().unwrap();
        if cx.src.line_of(end.start_offset()) - cx.src.line_of(p.keyword.start_offset()) <= min {
            return;
        }
        if cx.cfg.bool("AllowConsecutiveConditionals") == Some(true) {
            // the child before the node: the previous statement, or the parent if's condition
            let prev_if = match stmts.len() {
                1 => parent_is_if_with_cond.is_some_and(|c| if_parts(c).is_some()),
                n => if_parts(&stmts[n - 2]).is_some(),
            };
            if prev_if {
                return;
            }
        }
        Self::register(cx, &p, "return".into(), p.inverse, None);
        if let Some(s) = &p.if_branch {
            let inner: Vec<Node> = s.body().iter().collect();
            Self::check_ending_body(cx, &inner, Some(&p.pred));
        }
    }

    fn register(cx: &mut Cx, p: &If, scope: String, kw: &str, guard: Option<bool>) {
        let cond = node_text(&p.pred);
        let mut example = format!("{scope} {kw} {cond}");
        let start = p.node.location().start_offset();
        let max = helpers::alignment::cop_enabled(cx, "Layout/LineLength")
            .then(|| cx.config.for_cop("Layout/LineLength").int("Max").unwrap_or(120));
        let mut replacement = None;
        if max.is_some_and(|m| (cx.src.column(start) + example.chars().count()) as i64 > m) {
            let trivial = single(&p.if_branch).is_some_and(|b| {
                p.else_branch.is_none() && if_parts(&b).is_none() && b.as_parentheses_node().is_none()
            });
            if trivial {
                return;
            }
            example = format!("{kw} {cond}; {scope}; end");
            replacement = Some(format!("{kw} {cond}\n  {scope}\nend"));
        }
        let replacement = replacement.unwrap_or_else(|| example.clone());
        let (ks, ke) = rng(&p.keyword);
        let msg = format!("Use a guard clause (`{example}`) instead of wrapping the code inside a conditional expression.");
        let text = cx.src.text;
        let has_else = p.has_else();
        let pred_end = p.pred.location().end_offset();
        let then_kw = p.then_kw.as_ref().map(rng);
        let end_kw = p.end_kw.as_ref().map(rng);
        let else_kw = p.else_kw.as_ref().map(rng);
        let if_range = p.if_branch.as_ref().map(|s| rng(&s.location()));
        let else_range = p.else_branch.as_ref().map(|s| rng(&s.location()));
        let if_heredoc = find_heredoc(branch_first(&p.if_branch));
        let else_heredoc = if if_heredoc.is_none() { find_heredoc(branch_first(&p.else_branch)) } else { None };
        let remove_range = match guard {
            Some(true) => if_range,
            Some(false) => else_range,
            None => None,
        };
        cx.add_fix(ks, ke, msg, |c| {
            if has_else && guard.is_none() {
                return;
            }
            c.replace(ks, pred_end, &replacement);
            if let Some((s, e)) = then_kw {
                c.replace(s, e, "\n");
            }
            let Some((es, ee)) = end_kw else { return };
            let whole = |c: &mut Corrector, (s, e): R| {
                let (s, e) = range_by_whole_lines(text, s, e, true);
                c.remove(s, e);
            };
            let heredoc = if_heredoc.map(|h| (h, else_range)).or(else_heredoc.map(|h| (h, if_range)));
            if let Some((h, leave)) = heredoc {
                whole(c, (es, ee));
                if !has_else {
                    return;
                }
                if let Some((ls, le)) = leave {
                    whole(c, (ls, le));
                    c.insert_after(h.0, h.1, &format!("\n{}", &text[ls..le]));
                }
                if let Some(r) = else_kw {
                    whole(c, r);
                }
                if let Some(r) = remove_range {
                    whole(c, r);
                }
            } else {
                c.remove(es, ee);
                if !has_else {
                    return;
                }
                if let Some((s, e)) = else_kw {
                    c.remove(s, e);
                }
                if let Some((s, e)) = remove_range {
                    c.remove(s, e);
                }
            }
        });
    }
}

impl Cop for GuardClause {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE, kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match kind::of(node) {
            kind::DEF_NODE => {
                if let Some(stmts) = body_statements(node.as_def_node().unwrap().body()) {
                    Self::check_ending_body(cx, &stmts, None);
                }
            }
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                if !matches!(method_name(&call), "define_method" | "define_singleton_method") {
                    return;
                }
                let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
                if let Some(stmts) = body_statements(block.body()) {
                    Self::check_ending_body(cx, &stmts, None);
                }
            }
            _ => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                let p = if_parts(node).unwrap();
                let parent_assignment = helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(is_assignment);
                if Self::accepted_form(cx, &p, false, parent_assignment) {
                    return;
                }
                let (guard, kw, is_if) = if let Some(g) = guard_clause(&p.if_branch, cx.src) {
                    (g, text(&p.keyword), true)
                } else if let Some(g) = guard_clause(&p.else_branch, cx.src) {
                    (g, p.inverse, false)
                } else {
                    return;
                };
                let (gnode, and_or) = guard;
                let source = if and_or {
                    let b = if is_if { &p.if_branch } else { &p.else_branch };
                    node_text(&single(b).unwrap()).to_string()
                } else {
                    node_text(&gnode).to_string()
                };
                let kw = kw.to_string();
                Self::register(cx, &p, source, &kw, (!and_or).then_some(is_if));
            }
        }
    }
}
