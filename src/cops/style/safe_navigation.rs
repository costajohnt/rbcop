use crate::prelude::*;
use helpers::select_by::plain_send;
use crate::source::Source;
use helpers::parser_path::{Frame, Tag, ancestors, frame_of};
use helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use ruby_prism::Visit;

pub const NAME: &str = "Style/SafeNavigation";
pub static COP: SafeNavigation = SafeNavigation;
pub struct SafeNavigation;

const MSG: &str = "Use safe navigation (`&.`) instead of checking if an object exists before calling the method.";

type R = (usize, usize);

fn rng(n: &Node) -> R {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// `(send $_ :name)` with no arguments.
fn send_recv<'pr>(n: &Node<'pr>, names: &[&str]) -> Option<Node<'pr>> {
    let c = plain_send(n)?;
    if !names.contains(&method_name(&c)) || c.arguments().is_some() || c.block().is_some() {
        return None;
    }
    c.receiver()
}

/// `(send (send $_ :nil?) :!)`
fn not_nil_check<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    send_recv(&send_recv(n, &["!"])?, &["nil?"])
}

/// rubocop-ast `Node#receiver`: a call's receiver, or for a block its call's receiver.
fn receiver_of<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    n.as_call_node()?.receiver()
}

/// Parser `call_type?` (send/csend, not a block).
fn call_type<'pr>(n: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    n.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
}

fn args_of<'pr>(c: &ruby_prism::CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut v = call_args(c);
    if let Some(b) = c.block() {
        v.push(b);
    }
    v
}

fn matching_nodes(src: &Source, left: Option<&Node>, right: Option<&Node>) -> bool {
    let (Some(l), Some(r)) = (left, right) else { return left.is_none() && right.is_none() };
    if helpers::node_equality::same(src, rng(l), rng(r)) {
        return true;
    }
    let (Some(lc), Some(rc)) = (call_type(l), call_type(r)) else { return false };
    if method_name(&lc) != method_name(&rc) || !matching_nodes(src, lc.receiver().as_ref(), rc.receiver().as_ref()) {
        return false;
    }
    let (la, ra) = (args_of(&lc), args_of(&rc));
    la.len() == ra.len() && la.iter().zip(&ra).all(|(a, b)| helpers::node_equality::same(src, rng(a), rng(b)))
}

fn find_matching_receiver<'pr>(src: &Source, chain: &Node<'pr>, var: &Node) -> Option<Node<'pr>> {
    let r = receiver_of(chain);
    if matching_nodes(src, r.as_ref(), Some(var)) {
        return r;
    }
    find_matching_receiver(src, &r?, var)
}

/// `negated?` on `a[k]`, walking up through send parents.
fn negated(a: &[Frame], k: usize) -> bool {
    if a.get(k + 1).is_some_and(|p| p.tag == Tag::Send) {
        return negated(a, k + 1);
    }
    a[k].tag == Tag::Send && a[k].name == "!"
}

fn has_dot(f: &Frame) -> bool {
    f.dot.is_some_and(|d| d.2 == b".")
}

fn unsafe_method(a: &[Frame], k: usize, ternary: bool) -> bool {
    if negated(a, k) {
        return true;
    }
    !ternary && (a[k].setter || (!has_dot(&a[k]) && a[k].tag != Tag::Csend))
}

fn dotless_operator_method(f: &Frame) -> bool {
    f.dot.is_none() && helpers::node_types::is_operator_method(f.name)
}

struct Ctx<'c, 'a, 'pr> {
    cx: &'c Cx<'a, 'pr>,
    root: &'c Node<'pr>,
    ternary: bool,
}

impl<'pr> Ctx<'_, '_, 'pr> {
    fn src(&self) -> &Source<'_> {
        self.cx.src
    }

    /// `offending_node?` minus the matching check; `a` are the receiver's parser ancestors.
    fn offending(&self, unless_var_only: bool, chain: &Frame, a: &[Frame], chain_node: &Node) -> bool {
        if unless_var_only || a.first().is_none_or(|f| !f.is_call()) {
            return false;
        }
        let max = self.cx.cfg.int("MaxChainLength").unwrap_or(2) as usize;
        let mut len = 0;
        for f in a.iter().filter(|f| f.is_call()) {
            len += 1;
            if f.same(chain) {
                break;
            }
        }
        if len > max || self.unsafe_method_used(chain, a) {
            return false;
        }
        !plain_send(chain_node).is_some_and(|c| method_name(&c) == "empty?")
    }

    fn unsafe_method_used(&self, chain: &Frame, a: &[Frame]) -> bool {
        if unsafe_method(a, 0, self.ternary) {
            return true;
        }
        let chain_cop = helpers::alignment::cop_enabled(self.cx, "Lint/SafeNavigationChain");
        for j in 1..a.len() {
            if a[j].tag != Tag::Send {
                continue;
            }
            if !chain_cop || unsafe_method(a, j, self.ternary) || helpers::nil_methods::nil_method(self.cx.cfg, a[j].name) {
                return true;
            }
            if a[j].same(chain) {
                return false;
            }
        }
        false
    }

    /// `relevant_comment_ranges` + `each_comment_in_lines`: comment texts.
    fn comments(&self, children: &[R]) -> Vec<String> {
        let src = self.src();
        let (s, e) = rng(self.root);
        let mut begin = src.line_of(s);
        let mut ranges = Vec::new();
        for &(cs, ce) in children {
            ranges.push((begin, src.line_of(cs)));
            begin = src.line_of(ce);
        }
        ranges.push((begin, src.line_of(e)));
        let mut out = Vec::new();
        for (from, to) in ranges {
            for c in src.comments.iter().filter(|c| (from..to).contains(&src.line_of(c.start))) {
                out.push(src.text[c.start..c.end].to_string());
            }
        }
        out
    }
}

/// `add_safe_nav_to_all_methods_in_chain` over `a` (ancestors of the start node): dot insertion
/// points, or None when RuboCop's corrector would crash on a missing dot.
fn safe_nav_dots(a: &[Frame], chain: &Frame) -> Option<Vec<R>> {
    let mut out = Vec::new();
    for f in a {
        if !(f.is_call() || f.tag == Tag::Block) {
            break;
        }
        if f.tag != Tag::Send || helpers::node_types::is_operator_method(f.name) {
            continue;
        }
        let d = f.dot?;
        out.push((d.0, d.1));
        if f.same(chain) {
            break;
        }
    }
    Some(out)
}

struct Edits {
    removals: Vec<R>,
    replace: Option<(R, String)>,
    dots: Vec<R>,
    comments: Option<(R, String)>,
}

fn apply(c: &mut Corrector, e: &Edits) {
    for &(s, t) in &e.removals {
        c.remove(s, t);
    }
    if let Some(((s, t), text)) = &e.replace {
        c.replace(*s, *t, text);
    }
    if let Some(((s, t), text)) = &e.comments {
        c.insert_before(*s, *t, text);
    }
    for &(s, t) in &e.dots {
        c.insert_before(s, t, "&");
    }
}

/// Parser children (non-nil) of an if / ternary in order.
fn if_children(node: &Node) -> Vec<R> {
    let mut v = Vec::new();
    if let Some(i) = node.as_if_node() {
        v.push(rng(&i.predicate()));
        if let Some(s) = i.statements() {
            v.push(rng(&s.as_node()));
        }
        if let Some(s) = i.subsequent().and_then(|e| e.as_else_node()).and_then(|e| e.statements()) {
            v.push(rng(&s.as_node()));
        }
    } else if let Some(u) = node.as_unless_node() {
        v.push(rng(&u.predicate()));
        if let Some(s) = u.statements() {
            v.push(rng(&s.as_node()));
        }
    }
    v
}

/// A branch as the parser node it is: one statement, or the statements (a `begin`).
fn branch<'pr>(s: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Node<'pr>> {
    let s = s?;
    let mut it = s.body().iter();
    let first = it.next()?;
    Some(if it.next().is_none() { first } else { s.as_node() })
}

struct AndParts<'pr> {
    parts: Vec<Part<'pr>>,
    depth: usize,
}

enum Part<'pr> {
    Op(R),
    N(Node<'pr>),
}

impl Part<'_> {
    fn start(&self) -> usize {
        match self {
            Part::Op(r) => r.0,
            Part::N(n) => n.location().start_offset(),
        }
    }
}

/// `` `(begin and ...) ``: the node or a descendant is a begin whose first child is an `and`.
struct AndInBegin(bool);

impl<'pr> Visit<'pr> for AndInBegin {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let first = match &node {
            Node::ParenthesesNode { .. } => node.as_parentheses_node().unwrap().body().and_then(|b| match b.as_statements_node() {
                Some(s) => s.body().iter().next(),
                None => Some(b),
            }),
            Node::EmbeddedStatementsNode { .. } => {
                node.as_embedded_statements_node().unwrap().statements().and_then(|s| s.body().iter().next())
            }
            _ => None,
        };
        if first.is_some_and(|f| f.as_and_node().is_some()) {
            self.0 = true;
        }
    }
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        let mut it = node.body().iter();
        if it.next().is_some_and(|f| f.as_and_node().is_some()) && it.next().is_some() {
            self.0 = true;
        }
        ruby_prism::visit_statements_node(self, node);
    }
}

fn and_inside_begin(n: &Node) -> bool {
    let mut v = AndInBegin(false);
    v.visit(n);
    v.0
}

fn regular_block(b: Option<Node>) -> bool {
    b.and_then(|b| b.as_block_node().map(|bn| bn.parameters().is_none_or(|p| p.as_block_parameters_node().is_some())))
        .unwrap_or(false)
}

fn add_and_parts<'pr>(parts: &mut Vec<Part<'pr>>, n: &ruby_prism::AndNode<'pr>) {
    let op = n.operator_loc();
    parts.push(Part::Op((op.start_offset(), op.end_offset())));
    let (l, r) = (n.left(), n.right());
    if !and_inside_begin(&r) {
        parts.push(Part::N(r));
    }
    if l.as_and_node().is_none() && !and_inside_begin(&l) {
        parts.push(Part::N(l));
    }
}

impl<'pr> Visit<'pr> for AndParts<'pr> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'pr>) {
        if self.depth == 0 {
            add_and_parts(&mut self.parts, node);
        }
        ruby_prism::visit_and_node(self, node);
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let b = regular_block(node.block());
        self.depth += b as usize;
        ruby_prism::visit_call_node(self, node);
        self.depth -= b as usize;
    }
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'pr>) {
        let b = regular_block(node.block());
        self.depth += b as usize;
        ruby_prism::visit_super_node(self, node);
        self.depth -= b as usize;
    }
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode<'pr>) {
        let b = regular_block(node.block().map(|b| b.as_node()));
        self.depth += b as usize;
        ruby_prism::visit_forwarding_super_node(self, node);
        self.depth -= b as usize;
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        let b = node.parameters().is_none_or(|p| p.as_block_parameters_node().is_some()) as usize;
        self.depth += b;
        ruby_prism::visit_lambda_node(self, node);
        self.depth -= b;
    }
}

/// `each_ancestor(:block).any?` for the visited node (Prism ancestors).
fn outer_block(ancestors: &[Node]) -> bool {
    ancestors.iter().any(|a| match a {
        Node::CallNode { .. } => regular_block(a.as_call_node().unwrap().block()),
        Node::SuperNode { .. } => regular_block(a.as_super_node().unwrap().block()),
        Node::ForwardingSuperNode { .. } => regular_block(a.as_forwarding_super_node().unwrap().block().map(|b| b.as_node())),
        Node::LambdaNode { .. } => a.as_lambda_node().unwrap().parameters().is_none_or(|p| p.as_block_parameters_node().is_some()),
        _ => false,
    })
}

/// strip_begin: `{ (begin $!begin) $!(begin) }`: Ok(None) keeps the node, Err for `()`.
fn strip_begin<'pr>(n: &Node<'pr>) -> Result<Option<Node<'pr>>, ()> {
    let Some(p) = n.as_parentheses_node() else { return Ok(None) };
    let body = p.body().ok_or(())?;
    let only = match body.as_statements_node() {
        Some(s) => {
            let mut v: Vec<Node> = s.body().iter().collect();
            if v.len() != 1 {
                return Ok(None);
            }
            v.pop().unwrap()
        }
        None => body,
    };
    Ok(if only.as_parentheses_node().is_some() { None } else { Some(only) })
}

impl SafeNavigation {
    fn on_if(cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let (var, body, ternary, unless) = if let Some(i) = node.as_if_node() {
            if i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif") {
                return;
            }
            let ternary = i.if_keyword_loc().is_none();
            let else_branch = i.subsequent().and_then(|s| s.as_else_node()).and_then(|e| branch(e.statements()));
            if !ternary && i.subsequent().is_some() {
                return;
            }
            let if_branch = branch(i.statements());
            let pred = i.predicate();
            if ternary {
                let is_nil = |n: &Option<Node>| n.as_ref().is_some_and(|n| n.as_nil_node().is_some());
                if let (Some(v), true) = (send_recv(&pred, &["nil?", "!"]), is_nil(&if_branch)) {
                    (v, else_branch, true, false)
                } else if let (Some(v), true) = (not_nil_check(&pred), is_nil(&else_branch)) {
                    (v, if_branch, true, false)
                } else if is_nil(&else_branch) {
                    (pred, if_branch, true, false)
                } else {
                    return;
                }
            } else if if_branch.is_none() {
                // (if {(send $_ {:nil? :!}) $_} nil? $_) with an empty body: no receiver
                return;
            } else {
                (not_nil_check(&pred).unwrap_or(pred), if_branch, false, false)
            }
        } else {
            let u = node.as_unless_node().unwrap();
            if u.else_clause().is_some() {
                return;
            }
            let pred = u.predicate();
            (send_recv(&pred, &["nil?", "!"]).unwrap_or(pred), branch(u.statements()), false, true)
        };
        let Some(body) = body else { return };
        if matches!(kind::of(&body), kind::BREAK_NODE | kind::NEXT_NODE | kind::RETURN_NODE | kind::YIELD_NODE) {
            return;
        }
        let src = cx.src;
        let Some(receiver) = find_matching_receiver(src, &body, &var) else { return };
        let ctx = Ctx { cx, root: node, ternary };
        // use_var_only_in_unless_modifier?: the variable is the whole condition
        let pred_range = if_children(node)[0];
        let unless_var_only = unless && rng(&var) == pred_range;
        let Some(a) = ancestors(cx.ancestors, node, &receiver) else { return };
        let chain = frame_of(&body);
        if !ctx.offending(unless_var_only, &chain, &a, &body) {
            return;
        }
        // dotless_operator_call? / double_colon?
        if dotless_operator_method(&a[0]) {
            return;
        }
        let mut k = 0;
        while a.get(k + 1).is_some_and(|f| f.tag == Tag::Send) {
            k += 1;
        }
        if a.get(k + 1).is_none() || dotless_operator_method(&a[k]) || a[0].dot.is_some_and(|d| d.2 == b"::") {
            return;
        }
        let (ns, ne) = rng(node);
        let (bs, be) = chain.range;
        let mut dots = Vec::new();
        if a[0].tag != Tag::Csend {
            let Some(d) = a[0].dot else { return };
            dots.push((d.0, d.1));
        }
        let Some(more) = safe_nav_dots(&a[1..], &chain) else { return };
        dots.extend(more);
        let replace = var
            .as_call_node()
            .filter(is_csend)
            .map(|_| (rng(&receiver), node_text(&var).to_string()));
        let comments = ctx.comments(&if_children(node));
        let edits = Edits {
            removals: vec![(ns, bs), (be, ne)],
            replace,
            dots,
            comments: (!comments.is_empty()).then(|| (chain.range, format!("{}\n", comments.join("\n")))),
        };
        cx.add_fix(ns, ne, MSG, |c| apply(c, &edits));
    }

    fn on_and(cx: &mut Cx, node: &Node) {
        let and = node.as_and_node().unwrap();
        let mut parts = Vec::new();
        add_and_parts(&mut parts, &and);
        if !outer_block(cx.ancestors) {
            let mut v = AndParts { parts: Vec::new(), depth: 0 };
            // descendants only
            ruby_prism::visit_and_node(&mut v, &and);
            parts.extend(v.parts);
        }
        parts.sort_by_key(Part::start);
        let slices: Vec<&[Part]> = parts.chunks(2).collect();
        let src = cx.src;
        let rhs_or = {
            let r = and.right();
            r.as_or_node().is_some()
                || r.as_parentheses_node().and_then(|p| p.body()).is_some_and(|b| match b.as_statements_node() {
                    Some(s) => s.body().iter().count() == 1 && s.body().iter().next().unwrap().as_or_node().is_some(),
                    None => b.as_or_node().is_some(),
                })
        };
        let convert = cx.cfg.bool("ConvertCodeThatCanStartToReturnNil") == Some(true);
        let mut ignored = false;
        for w in slices.windows(2) {
            let (Part::N(lhs), Part::N(rhs)) = (&w[0][0], &w[1][0]) else { continue };
            let lhs_op = w[0].get(1);
            let not_nil = not_nil_check(lhs);
            let lhs_receiver: &Node = not_nil.as_ref().unwrap_or(lhs);
            let rhs_receiver = match strip_begin(rhs) {
                Ok(Some(s)) => find_matching_receiver(src, &s, lhs_receiver),
                Ok(None) => find_matching_receiver(src, rhs, lhs_receiver),
                Err(()) => None,
            };
            if !convert && not_nil.is_some() {
                continue;
            }
            let Some(rhs_receiver) = rhs_receiver else { continue };
            let ctx = Ctx { cx, root: node, ternary: false };
            let Some(a) = ancestors(cx.ancestors, node, &rhs_receiver) else { continue };
            let chain = frame_of(rhs);
            if !ctx.offending(false, &chain, &a, rhs) {
                continue;
            }
            if not_nil.is_none() {
                let parent_call = ancestors(cx.ancestors, node, lhs_receiver).is_some_and(|p| p.first().is_some_and(Frame::is_call));
                if parent_call {
                    continue;
                }
            }
            // range_with_surrounding_space on a node operator range crashes the cop
            let Some(Part::Op(op)) = lhs_op else { return };
            let right = Surrounding { side: Side::Right, ..Default::default() };
            let (ls, le) = rng(lhs);
            let r1 = range_with_surrounding_space(src.text, ls, le, right);
            let r2 = range_with_surrounding_space(src.text, op.0, op.1, right);
            let (_, re) = rng(rhs);
            if ignored || rhs_or {
                cx.add_fix(ls, re, MSG, |_| {});
            } else {
                let Some(dots) = safe_nav_dots(&a, &chain) else { return };
                let comments = ctx.comments(&[rng(&and.left()), rng(&and.right())]);
                let edits = Edits {
                    removals: vec![r1, r2],
                    replace: Some((rng(&rhs_receiver), node_text(lhs_receiver).to_string())),
                    dots,
                    comments: (!comments.is_empty()).then(|| (chain.range, format!("{}\n", comments.join("\n")))),
                };
                cx.add_fix(ls, re, MSG, |c| apply(c, &edits));
            }
            ignored = true;
        }
    }
}

impl Cop for SafeNavigation {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.3, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::AND_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if node.as_and_node().is_some() {
            Self::on_and(cx, node);
        } else {
            Self::on_if(cx, node);
        }
    }
}
