use crate::helpers::if_node::parser_end;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/DoubleNegation";
pub static COP: DoubleNegation = DoubleNegation;
pub struct DoubleNegation;

/// A parser-gem ancestor of the `!!` node, as far as this cop cares.
enum P<'a, 'pr> {
    Return,
    Def(&'a Node<'pr>),
    /// A block whose send is `define_method`/`define_singleton_method`: that send.
    DefineMethod(ruby_prism::CallNode<'pr>),
    /// if / while / until / case / case_match, with its parser end offset.
    Conditional(usize),
    Enumerable,
    /// begin, with its end offset.
    Begin(usize),
    Other,
}

fn is_define_method(call: &ruby_prism::CallNode) -> bool {
    !is_csend(call) && matches!(method_name(call), "define_method" | "define_singleton_method")
}

/// Parser-shaped ancestors, nearest first.
fn parser_parents<'a, 'pr>(ancestors: &'a [Node<'pr>]) -> Vec<P<'a, 'pr>> {
    let mut out = Vec::new();
    for i in (0..ancestors.len()).rev() {
        let n = &ancestors[i];
        let parent = i.checked_sub(1).map(|j| &ancestors[j]);
        match kind::of(n) {
            kind::PROGRAM_NODE | kind::ARGUMENTS_NODE | kind::ELSE_NODE => {}
            kind::STATEMENTS_NODE => {
                let s = n.as_statements_node().unwrap();
                let wrapped = parent.is_some_and(|p| {
                    matches!(kind::of(p), kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE)
                        || p.as_begin_node().is_some_and(|b| b.begin_keyword_loc().is_some())
                });
                if s.body().iter().count() > 1 && !wrapped {
                    out.push(P::Begin(n.location().end_offset()));
                }
            }
            kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE => out.push(P::Begin(n.location().end_offset())),
            kind::CALL_NODE => {
                let call = n.as_call_node().unwrap();
                let block = call.block().filter(|b| b.as_block_node().is_some());
                let inside_block = block.is_some() && ancestors.get(i + 1).is_some_and(|c| kind::of(c) == kind::BLOCK_NODE);
                if inside_block {
                    continue;
                }
                out.push(P::Other);
                if block.is_some() {
                    out.push(if is_define_method(&call) { P::DefineMethod(call) } else { P::Other });
                }
            }
            kind::BLOCK_NODE => {
                let call = parent.and_then(|p| p.as_call_node()).filter(is_define_method);
                out.push(call.map_or(P::Other, P::DefineMethod));
            }
            kind::DEF_NODE => out.push(P::Def(n)),
            kind::IF_NODE | kind::UNLESS_NODE => {
                let guard = parent.and_then(|p| p.as_in_node()).is_some_and(|inn| {
                    inn.pattern().location().start_offset() == n.location().start_offset()
                });
                out.push(if guard { P::Other } else { P::Conditional(parser_end(n)) });
            }
            kind::WHILE_NODE if n.as_while_node().unwrap().is_begin_modifier() => out.push(P::Other),
            kind::UNTIL_NODE if n.as_until_node().unwrap().is_begin_modifier() => out.push(P::Other),
            kind::WHILE_NODE | kind::UNTIL_NODE | kind::CASE_NODE | kind::CASE_MATCH_NODE => {
                out.push(P::Conditional(n.location().end_offset()))
            }
            kind::ASSOC_NODE | kind::HASH_NODE | kind::KEYWORD_HASH_NODE | kind::ARRAY_NODE => out.push(P::Enumerable),
            kind::RETURN_NODE => out.push(P::Return),
            _ => out.push(P::Other),
        }
    }
    out
}

/// A parser node's range and whether it is a pair/hash.
#[derive(Clone, Copy)]
struct Child {
    start: usize,
    end: usize,
    pair_or_hash: bool,
}

fn child(node: &Node) -> Child {
    let loc = node.location();
    let pair_or_hash = matches!(kind::of(node), kind::ASSOC_NODE | kind::HASH_NODE | kind::KEYWORD_HASH_NODE);
    Child { start: loc.start_offset(), end: parser_end(node), pair_or_hash }
}

/// The parser node for a statements list: the statement itself, or a `begin` around several.
fn statements_child(s: &ruby_prism::StatementsNode) -> Option<Child> {
    let list: Vec<Node> = s.body().iter().collect();
    match list.as_slice() {
        [] => None,
        [one] => Some(child(one)),
        [first, .., last] => {
            Some(Child { start: first.location().start_offset(), end: last.location().end_offset(), pair_or_hash: false })
        }
    }
}

/// Direct children at depth 1 (Prism-only wrappers are not entered, so their children count).
struct Children<'pr> {
    depth: usize,
    /// Whether each entered branch node increased the depth (an ElseNode is transparent).
    stack: Vec<bool>,
    out: Vec<Node<'pr>>,
}

impl<'pr> Visit<'pr> for Children<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let transparent = self.depth == 1 && node.as_else_node().is_some();
        if self.depth == 1 && !transparent {
            self.out.push(node);
        }
        if !transparent {
            self.depth += 1;
        }
        self.stack.push(!transparent);
    }
    fn visit_branch_node_leave(&mut self) {
        if self.stack.pop() == Some(true) {
            self.depth -= 1;
        }
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        if self.depth == 1 {
            self.out.push(node);
        }
    }
}

/// `node.child_nodes.last` of a parser node that Prism represents as `node`.
fn last_child_of(node: &Node) -> Option<Child> {
    if let Some(call) = node.as_call_node() {
        if let Some(block) = call.block().and_then(|b| b.as_block_node()) {
            if let Some(body) = block.body() {
                return Some(match body.as_statements_node() {
                    Some(s) => statements_child(&s)?,
                    None => {
                        let (s, e) = body_range(&body);
                        Child { start: s, end: e, pair_or_hash: false }
                    }
                });
            }
            return match block.parameters() {
                Some(p) if p.as_block_parameters_node().is_some() => Some(child(&p)),
                Some(_) => {
                    let (s, e) = send_range(&call);
                    Some(Child { start: s, end: e, pair_or_hash: false })
                }
                None => None, // `(args)` without a location: RuboCop crashes
            };
        }
        if let Some(last) = send_args(&call).last() {
            return Some(child(last));
        }
        return call.receiver().map(|r| child(&r));
    }
    if let Some(n) = node.as_if_node() {
        if let Some(sub) = n.subsequent() {
            match sub.as_else_node() {
                Some(e) => {
                    if let Some(c) = e.statements().as_ref().and_then(statements_child) {
                        return Some(c);
                    }
                }
                None => return Some(child(&sub)),
            }
        }
        if let Some(c) = n.statements().as_ref().and_then(statements_child) {
            return Some(c);
        }
        return Some(child(&n.predicate()));
    }
    if let Some(n) = node.as_unless_node() {
        if let Some(c) = n.statements().as_ref().and_then(statements_child) {
            return Some(c);
        }
        if let Some(c) = n.else_clause().and_then(|e| e.statements()).as_ref().and_then(statements_child) {
            return Some(c);
        }
        return Some(child(&n.predicate()));
    }
    let mut v = Children { depth: 0, stack: Vec::new(), out: Vec::new() };
    v.visit(node);
    v.out.iter().max_by_key(|n| n.location().end_offset()).map(child)
}

/// `find_last_child(def_node.body)` for a def; None where RuboCop would crash.
fn def_last_child(def: &ruby_prism::DefNode) -> Option<(Child, bool)> {
    let body = def.body()?;
    let stmts = if let Some(b) = body.as_begin_node() {
        match (b.statements(), b.rescue_clause(), b.ensure_clause()) {
            (Some(s), _, _) => s,
            (None, None, Some(e)) => e.statements()?,
            _ => return None,
        }
    } else {
        body.as_statements_node()?
    };
    let list: Vec<Node> = stmts.body().iter().collect();
    match list.as_slice() {
        [] => None,
        [one] => Some((last_child_of(one)?, one.as_array_node().is_some())),
        [.., last] => Some((child(last), false)),
    }
}

impl Cop for DoubleNegation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "!" || is_csend(&call) {
            return;
        }
        let Some(sel) = call.message_loc().filter(|s| s.as_slice() == b"!") else { return };
        let inner = call.receiver().and_then(|r| r.as_call_node());
        if !inner.is_some_and(|i| method_name(&i) == "!" && !is_csend(&i)) {
            return;
        }
        let src = cx.src;
        let (ns, ne) = send_range(&call);
        if cx.cfg.str("EnforcedStyle") != Some("forbidden") {
            let parents = parser_parents(cx.ancestors);
            if matches!(parents.first(), Some(P::Return)) {
                return;
            }
            match end_of_method_definition(src, ns, &parents) {
                Some(true) | None => return,
                Some(false) => {}
            }
        }
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(ss, se, "Avoid the use of double negation (`!!`).", |c| {
            c.remove(ss, se);
            c.insert_after(ns, ne, ".nil?");
        });
    }
}

/// `end_of_method_definition?`; None where RuboCop crashes (no offense either way).
fn end_of_method_definition(src: &crate::source::Source, node_start: usize, parents: &[P]) -> Option<bool> {
    let line = |off: usize| src.line_of(off);
    let def = parents.iter().find(|p| matches!(p, P::Def(_) | P::DefineMethod(_)));
    let (last, parent_array) = match def {
        None => return Some(false),
        Some(P::Def(d)) => def_last_child(&d.as_def_node().unwrap())?,
        Some(P::DefineMethod(send)) => {
            let args = send_args(send);
            let last = match args.last() {
                Some(a) => child(a),
                None => child(&send.receiver()?),
            };
            (last, false)
        }
        Some(_) => unreachable!(),
    };
    let conditional = parents.iter().find_map(|p| if let P::Conditional(end) = p { Some(*end) } else { None });
    if let Some(cond_end) = conditional {
        let parent = parents.iter().find(|p| !matches!(p, P::Enumerable))?;
        return Some(match parent {
            P::Begin(end) => line(node_start) == line(end - 1),
            _ => line(last.end.saturating_sub(1).max(last.start)) <= line(cond_end - 1),
        });
    }
    if last.pair_or_hash || parent_array {
        return Some(false);
    }
    Some(line(last.start) <= line(node_start))
}
