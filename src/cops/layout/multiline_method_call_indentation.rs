use crate::helpers::alignment as alignment;
use crate::helpers::multiline_expression_indentation::{Ctx, PAnc, PK, SELF, block_of, indentation, within};
use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Layout/MultilineMethodCallIndentation";
pub static COP: MultilineMethodCallIndentation = MultilineMethodCallIndentation;
pub struct MultilineMethodCallIndentation;

const OPERATORS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

/// `@hash_pair_base_column`: set by the `indented` hash pair check and never reset in a file.
#[derive(Default)]
struct State {
    hash_pair_base_column: Option<i64>,
}

type Range = (usize, usize);

fn span(n: &Node) -> Range {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// `loc.dot.join(loc.selector)` (None without a dot or selector).
fn dot_sel(call: &CallNode) -> Option<Range> {
    Some((call.call_operator_loc()?.start_offset(), call.message_loc()?.end_offset()))
}

fn has_block(n: &Node) -> bool {
    n.as_call_node().is_some_and(|c| block_of(&c).is_some())
}

/// any_block_type? for a parser node standing for this Prism node.
fn is_any_block(n: &Node) -> bool {
    has_block(n) || kind::of(n) == kind::LAMBDA_NODE
}

/// A send in parser terms: a CallNode without a `{}`/`do` block (else the parser node is the block).
fn as_send<'pr>(n: &Node<'pr>) -> Option<CallNode<'pr>> {
    n.as_call_node().filter(|c| block_of(c).is_none())
}

/// What `semantic_alignment_node` and friends return: nothing, or a node (whose dot+selector, if
/// it has both, is the alignment base).
enum Sem {
    Nil,
    Node(Option<Range>),
}

struct Check<'a, 'b, 'pr> {
    cx: &'a Cx<'b, 'pr>,
    ctx: Ctx<'a, 'pr>,
    call: CallNode<'pr>,
    /// The receiver chain: `chain[0]` is the node, `chain[i + 1]` the receiver of `chain[i]`.
    chain: Vec<Node<'pr>>,
}

impl<'a, 'b, 'pr> Check<'a, 'b, 'pr> {
    fn line(&self, off: usize) -> usize {
        self.cx.src.line_of(off)
    }
    fn col(&self, off: usize) -> i64 {
        self.cx.src.column(off) as i64
    }
    fn text(&self, r: Range) -> &str {
        self.cx.src.slice(r.0, r.1)
    }
    fn chain_call(&self, i: usize) -> Option<CallNode<'pr>> {
        self.chain.get(i)?.as_call_node()
    }

    /// first_call_has_a_dot: index into the chain of the lowest call with a dot.
    fn first_call_with_dot(&self) -> usize {
        let base = self.chain.len() - 1;
        (0..base)
            .rev()
            .find(|&i| self.chain_call(i).is_some_and(|c| c.call_operator_loc().is_some()))
            .unwrap_or(0)
    }

    fn base_receiver(&self) -> &Node<'pr> {
        self.chain.last().unwrap()
    }

    /// method_on_receiver_last_line?(chain[i], base, type)
    fn method_on_receiver_last_line(&self, call: &CallNode, base: &Node, kind_: usize) -> bool {
        call.call_operator_loc().is_some_and(|d| self.line(d.start_offset()) == self.line(span(base).1))
            && kind::of(base) == kind_
    }

    fn rhs_starts_with_dot(&self, rhs: Range) -> bool {
        let t = self.text(rhs);
        t.starts_with('.') || t.starts_with("&.")
    }

    /// find_pair_ancestor: index into the parser ancestors.
    fn pair_ancestor(&self) -> Option<usize> {
        for (j, a) in self.ctx.anc.iter().enumerate() {
            if a.kind == PK::Pair {
                return Some(j);
            }
            if a.kind == PK::Begin || self.ctx.inside_arg_list_parentheses(a) {
                return None;
            }
            // inside_body?(ancestor, child): a block body (not its send: the previous parser
            // ancestor from the same Prism node, or the node itself), a non-ternary `if`, a
            // `kwbegin` or a loop.
            let inside_body = match a.kind {
                PK::Block | PK::OtherBlock => {
                    if j == 0 { a.idx != SELF } else { self.ctx.anc[j - 1].idx != a.idx }
                }
                PK::If => !self.ctx.is_ternary(a),
                PK::KwBegin | PK::While | PK::Until | PK::For => true,
                _ => false,
            };
            if inside_body {
                return None;
            }
        }
        None
    }

    fn inside_multiline_chain_arg(&self) -> bool {
        let Some(j) = self.pair_ancestor() else { return false };
        let (Some(hash), Some(enclosing)) = (self.ctx.anc.get(j + 1), self.ctx.anc.get(j + 2)) else { return false };
        if !matches!(enclosing.kind, PK::Send | PK::Csend) {
            return false;
        }
        let Some(call) = self.ctx.call(enclosing) else { return false };
        let Some(receiver) = call.receiver() else { return false };
        if span(&receiver) == (hash.start, hash.end) || call.call_operator_loc().is_none() {
            return false;
        }
        call.message_loc().is_none_or(|s| self.line(s.start_offset()) != self.line(span(&receiver).0))
    }

    fn aligned_with_first_line_dot(&self, rhs: Range) -> bool {
        if !self.rhs_starts_with_dot(rhs) {
            return false;
        }
        let k = self.first_call_with_dot();
        if k == 1 && !has_block(&self.chain[1]) {
            return false;
        }
        let Some(dot) = self.chain_call(k).and_then(|c| c.call_operator_loc()) else { return false };
        self.line(dot.start_offset()) == self.line(span(&self.chain[0]).0) && self.col(dot.start_offset()) == self.col(rhs.0)
    }

    fn first_dot_alignment_base(&self, rhs: Range) -> Option<Range> {
        if !self.rhs_starts_with_dot(rhs) {
            return None;
        }
        let k = self.first_call_with_dot();
        let first = self.chain_call(k)?;
        let dot = first.call_operator_loc()?;
        if k == 0 {
            return None;
        }
        // after_multiline_block_base
        if block_of(&first).is_some()
            && block_multiline(&self.chain[k])
            && k > 1
            && let Some(after) = self.chain_call(k - 1)
            && after.call_operator_loc().is_some()
        {
            return dot_sel(&after);
        }
        let receiver = first.receiver()?;
        if self.line(dot.start_offset()) != self.line(span(&receiver).0) {
            return None;
        }
        dot_sel(&first)
    }

    fn get_dot_right_above(&self) -> Sem {
        let Some(dot) = self.call.call_operator_loc() else { return Sem::Nil };
        let (line, col) = (self.line(dot.start_offset()), self.col(dot.start_offset()));
        for a in &self.ctx.anc {
            if !matches!(a.kind, PK::Send | PK::Csend) {
                continue;
            }
            let Some(c) = self.ctx.call(a) else { continue };
            let Some(d) = c.call_operator_loc() else { continue };
            if self.line(d.start_offset()) + 1 == line && self.col(d.start_offset()) == col {
                return Sem::Node(dot_sel(&c));
            }
        }
        Sem::Nil
    }

    fn single_line_block_receiver(&self, receiver: &Node) -> bool {
        is_any_block(receiver) && !block_multiline(receiver)
    }

    /// leftmost_call_on_same_line for a block receiver.
    fn leftmost_call_on_same_line(&self, node: &Node<'pr>) -> Sem {
        let Some(mut current) = node.as_call_node() else { return Sem::Node(None) };
        while let Some(receiver) = current.receiver().and_then(|r| r.as_call_node()) {
            let (Some(rd), Some(cd)) = (receiver.call_operator_loc(), current.call_operator_loc()) else { break };
            if self.line(rd.start_offset()) != self.line(cd.start_offset()) {
                break;
            }
            current = receiver;
        }
        Sem::Node(dot_sel(&current))
    }

    fn find_continuation_node(&self) -> Sem {
        let Some(receiver) = self.call.receiver() else { return Sem::Nil };
        if self.single_line_block_receiver(&receiver) {
            return self.leftmost_call_on_same_line(&receiver);
        }
        let Some(rc) = as_send(&receiver) else { return Sem::Nil };
        let Some(dot) = rc.call_operator_loc() else { return Sem::Nil };
        let Some(rr) = rc.receiver() else { return Sem::Nil };
        if kind::of(&rr) == kind::PARENTHESES_NODE && !block_multiline(&self.chain[0]) {
            return Sem::Node(dot_sel(&rc));
        }
        if self.line(dot.start_offset()) <= self.line(span(&rr).1) {
            return Sem::Nil;
        }
        Sem::Node(dot_sel(&rc))
    }

    fn handle_descendant_block(&self) -> Sem {
        let Some(receiver) = self.call.receiver() else { return Sem::Nil };
        if self.single_line_block_receiver(&receiver) {
            return self.leftmost_call_on_same_line(&receiver);
        }
        let mut finder = BlockFinder { stack: Vec::new(), found: None };
        finder.visit(&self.chain[0]);
        let Some((multiline, parent)) = finder.found else { return Sem::Nil };
        if !multiline {
            return Sem::Nil;
        }
        if let Some(rc) = as_send(&receiver) {
            return Sem::Node(dot_sel(&rc));
        }
        Sem::Node(parent)
    }

    fn first_call_alignment_node(&self) -> Sem {
        let k = self.first_call_with_dot();
        let Some(call) = self.chain_call(k) else { return Sem::Nil };
        let base = self.base_receiver();
        if self.method_on_receiver_last_line(&call, base, kind::ARRAY_NODE) {
            return Sem::Node(dot_sel(&call));
        }
        let Some(dot) = call.call_operator_loc() else { return Sem::Nil };
        if self.line(dot.start_offset()) != self.line(span(&self.chain[k]).0) {
            return Sem::Nil;
        }
        if self.method_on_receiver_last_line(&call, base, kind::PARENTHESES_NODE) {
            return Sem::Nil;
        }
        Sem::Node(dot_sel(&call))
    }

    fn semantic_alignment_base(&self, rhs: Range) -> Option<Range> {
        if !self.rhs_starts_with_dot(rhs) || self.ctx.argument_in_method_call(true).is_some() {
            return None;
        }
        let candidates: [&dyn Fn() -> Sem; 3] = [
            &|| self.get_dot_right_above(),
            &|| {
                if block_of(&self.call).is_some() {
                    self.find_continuation_node()
                } else {
                    self.handle_descendant_block()
                }
            },
            &|| self.first_call_alignment_node(),
        ];
        for f in candidates {
            if let Sem::Node(r) = f() {
                return r;
            }
        }
        None
    }

    fn syntactic_alignment_base(&self, rhs: Range) -> Option<Range> {
        if let Some(kw) = self.ctx.keyword_node() {
            return self.ctx.keyword_expression(kw);
        }
        if let Some(a) = self.ctx.part_of_assignment_rhs(Some(rhs)) {
            return self.ctx.assignment_rhs(a);
        }
        // operation_rhs: a send ancestor of the receiver that is an operator with it in its first argument.
        let receiver = span(&self.call.receiver()?);
        let own = (!is_csend(&self.call)).then(|| self.call.as_node());
        let own_iter = own.iter().filter_map(|n| n.as_call_node());
        let anc_iter = self.ctx.anc.iter().filter(|a| a.kind == PK::Send).filter_map(|a| self.ctx.call(a));
        for c in own_iter.chain(anc_iter) {
            if !OPERATORS.contains(&method_name(&c)) {
                continue;
            }
            let Some(first) = send_args(&c).into_iter().next() else { continue };
            if within(receiver, span(&first)) {
                return Some(span(&first));
            }
        }
        None
    }

    fn receiver_alignment_base(&self) -> Option<Range> {
        // find_hash_method_base_in_receiver_chain
        let mut chain = self.call.receiver().and_then(|r| r.as_call_node());
        while let Some(rc) = chain {
            let base = rc.receiver();
            let hit = base.as_ref().is_some_and(|b| {
                kind::of(b) == kind::HASH_NODE || self.method_on_receiver_last_line(&rc, b, kind::PARENTHESES_NODE)
            });
            if hit {
                return dot_sel(&rc);
            }
            chain = base.and_then(|b| b.as_call_node());
        }
        let first = self.chain_call(self.first_call_with_dot())?;
        Some(span(&first.receiver()?))
    }

    /// What `autocorrect(corrector, rhs)` hands AlignmentCorrector.correct: the rhs range, or
    /// with a block the selector's line, then each line of the block body and the `end` line
    /// other than the selector's (`correct_block`).
    fn correction_targets(&self, rhs: Range) -> Vec<(usize, usize, Option<Node<'pr>>)> {
        let Some(block) = block_of(&self.call) else { return vec![(rhs.0, rhs.1, None)] };
        let src = self.cx.src;
        let line_range = |line: usize| {
            let start = src.line_start(line);
            (start, start + src.lines.get(line - 1).map_or(0, |l| l.len()), None)
        };
        let selector_line = src.line_of(rhs.0);
        let mut lines: Vec<usize> = match block.body() {
            Some(body) => {
                let (s, e) = crate::ast::body_range(&body);
                (src.line_of(s)..=src.line_of(e)).collect()
            }
            None => Vec::new(),
        };
        let end_line = src.line_of(block.closing_loc().start_offset());
        if !lines.contains(&end_line) {
            lines.push(end_line);
        }
        let mut v = vec![line_range(selector_line)];
        v.extend(lines.into_iter().filter(|&l| l != selector_line).map(line_range));
        v
    }
}

/// First block (any_block) below the node in parser pre-order: whether it spans several lines,
/// and the dot+selector of its parser parent if that is a call with both.
struct BlockFinder<'pr> {
    stack: Vec<Node<'pr>>,
    found: Option<(bool, Option<Range>)>,
}

impl<'pr> Visit<'pr> for BlockFinder<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        if self.found.is_none() && !self.stack.is_empty() {
            let block = match kind::of(&node) {
                kind::CALL_NODE => has_block(&node),
                kind::LAMBDA_NODE => true,
                kind::SUPER_NODE => node.as_super_node().unwrap().block().is_some_and(|b| b.as_block_node().is_some()),
                kind::FORWARDING_SUPER_NODE => node.as_forwarding_super_node().unwrap().block().is_some(),
                _ => false,
            };
            if block {
                let parent = self.stack.iter().rev().find(|n| kind::of(n) != kind::ARGUMENTS_NODE);
                let sel = parent.and_then(|p| p.as_call_node()).and_then(|c| dot_sel(&c));
                self.found = Some((block_multiline(&node), sel));
            }
        }
        self.stack.push(node);
    }
    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }
}

/// BlockNode#multiline?: the `{`/`do` and `}`/`end` are on different lines.
fn block_multiline(node: &Node) -> bool {
    let block = match kind::of(node) {
        kind::CALL_NODE => node.as_call_node().unwrap().block(),
        kind::SUPER_NODE => node.as_super_node().unwrap().block(),
        kind::FORWARDING_SUPER_NODE => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        _ => None,
    };
    let (open, close) = match (block.as_ref().and_then(|b| b.as_block_node()), node.as_lambda_node()) {
        (Some(b), _) => (b.opening_loc(), b.closing_loc()),
        (None, Some(l)) => (l.opening_loc(), l.closing_loc()),
        _ => return node.location().as_slice().contains(&b'\n'),
    };
    let start = node.location().start_offset();
    node.location().as_slice()[open.start_offset() - start..close.start_offset() - start].contains(&b'\n')
}

impl Cop for MultilineMethodCallIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(receiver) = call.receiver() else { return };
        if method_name(&call) == "[]" {
            return;
        }
        let Some(dot) = call.call_operator_loc() else { return };
        let src = cx.src;
        // right_hand_side
        let dot_text = dot.as_slice();
        let rhs = match call.message_loc() {
            Some(sel) if matches!(dot_text, b"." | b"&.") && src.line_of(dot.start_offset()) == src.line_of(sel.start_offset()) => {
                (dot.start_offset(), sel.end_offset())
            }
            Some(sel) => (sel.start_offset(), sel.end_offset()),
            None if method_name(&call) == "call" => match call.opening_loc() {
                Some(open) => (dot.start_offset(), open.end_offset()),
                None => return,
            },
            None => return,
        };
        // Each way out without an offense below is `correct_style_detected` (offending_range is
        // falsy).
        if !alignment::begins_its_line(src, rhs.0) {
            return cx.correct_style_detected();
        }
        let range = send_range(&call);
        let ctx = Ctx::new(cx.ancestors, node, range);
        // left_hand_side
        let assignment_method = |c: &CallNode| {
            let n = method_name(c);
            n.ends_with('=') && !matches!(n, "==" | "===" | "!=" | "<=" | ">=")
        };
        let lhs = if assignment_method(&call) {
            span(&receiver)
        } else {
            let mut lhs = range;
            for a in &ctx.anc {
                match ctx.call(a) {
                    Some(c)
                        if matches!(a.kind, PK::Send | PK::Csend)
                            && c.call_operator_loc().is_some()
                            && !assignment_method(&c) =>
                    {
                        lhs = (a.start, a.end)
                    }
                    _ => break,
                }
            }
            lhs
        };
        let mut chain = vec![node.as_call_node().unwrap().as_node()];
        let mut cur = call.receiver();
        while let Some(r) = cur {
            cur = r.as_call_node().and_then(|c| c.receiver());
            chain.push(r);
        }
        let width = alignment::configured_indentation_width(cx);
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("aligned").to_string();
        let check = Check { cx, ctx, call, chain };
        let pair = check.pair_ancestor();
        let mut base: Option<Range> = None;
        let mut new_hpbc = None;
        let delta = if let Some(j) = pair.filter(|_| style == "aligned") {
            let _ = j;
            // check_hash_pair_indentation
            let hash_base = (kind::of(check.base_receiver()) == kind::HASH_NODE && check.chain.len() > 1)
                .then(|| check.chain_call(check.first_call_with_dot()).and_then(|c| dot_sel(&c)))
                .flatten();
            if hash_base.is_none() && check.inside_multiline_chain_arg() {
                return check.cx.correct_style_detected();
            }
            let b = hash_base.or_else(|| check.first_dot_alignment_base(rhs)).unwrap_or(lhs);
            base = Some(b);
            if check.aligned_with_first_line_dot(rhs) {
                return check.cx.correct_style_detected();
            }
            check.col(b.0) - check.col(rhs.0)
        } else if let Some(j) = pair.filter(|_| style == "indented" && kind::of(check.base_receiver()) == kind::HASH_NODE) {
            let key = check.ctx.prism(&check.ctx.anc[j]).as_assoc_node().unwrap().key();
            let key_col = check.col(span(&key).0);
            new_hpbc = Some(key_col + width);
            key_col + 2 * width - check.col(rhs.0)
        } else {
            if if pair.is_some() { check.inside_multiline_chain_arg() } else { check.ctx.not_for_this_cop() } {
                return check.cx.correct_style_detected();
            }
            // check_regular_indentation
            base = match style.as_str() {
                "aligned" => check.semantic_alignment_base(rhs).or_else(|| check.syntactic_alignment_base(rhs)),
                "indented_relative_to_receiver" => check.receiver_alignment_base(),
                _ => None,
            };
            let correct = match base {
                Some(b) => {
                    let mut parent: Option<&PAnc> = check.ctx.anc.first();
                    if parent.is_some_and(|p| matches!(p.kind, PK::Block | PK::OtherBlock)) {
                        parent = check.ctx.anc.get(1);
                    }
                    let extra = if style != "indented_relative_to_receiver" {
                        0
                    } else {
                        match parent.map(|p| p.kind) {
                            Some(PK::Splat) => width - 1,
                            Some(PK::KwSplat) => width - 2,
                            _ => width,
                        }
                    };
                    check.col(b.0) + extra
                }
                None => indentation(cx, lhs.0) + check.ctx.correct_indentation(cx, width),
            };
            correct - check.col(rhs.0)
        };
        if delta == 0 {
            drop(check);
            if let Some(h) = new_hpbc {
                cx.state::<State>().hash_pair_base_column = Some(h);
            }
            return cx.correct_style_detected();
        }
        let rhs_src = check.text(rhs).to_string();
        let msg = match base {
            Some(b) if style == "indented_relative_to_receiver" || style == "aligned" => {
                let base_src = check.text(b).split('\n').next().unwrap_or("").to_string();
                let line = check.line(b.0);
                if style == "aligned" {
                    format!("Align `{rhs_src}` with `{base_src}` on line {line}.")
                } else {
                    format!("Indent `{rhs_src}` {width} spaces more than `{base_src}` on line {line}.")
                }
            }
            _ => {
                let hpbc = new_hpbc.or_else(|| {
                    let cx: &Cx = check.cx;
                    cx.state.as_ref().and_then(|s| s.downcast_ref::<State>()).and_then(|s| s.hash_pair_base_column)
                });
                let (used, expected) = match hpbc {
                    Some(h) => (check.col(rhs.0) - h, width),
                    None => (check.col(rhs.0) - indentation(cx, lhs.0), check.ctx.correct_indentation(cx, width)),
                };
                let what = check.ctx.operation_description(Some(rhs));
                format!("Use {expected} (not {used}) spaces for indenting {what} spanning multiple lines.")
            }
        };
        let targets = check.correction_targets(rhs);
        drop(check);
        if let Some(h) = new_hpbc {
            cx.state::<State>().hash_pair_base_column = Some(h);
        }
        let (src, set) = (cx.src, helpers::alignment_corrector::Settings::of(cx));
        // incorrect_style_detected: more than two supported styles.
        if cx.block_runs(rhs.0, rhs.1) {
            cx.no_acceptable_style();
        }
        cx.add_fix(rhs.0, rhs.1, msg, |c| {
            for (s, e, node) in &targets {
                helpers::alignment_corrector::correct(c, src, set, *s, *e, node.as_ref(), delta, false);
            }
        });
    }
}
