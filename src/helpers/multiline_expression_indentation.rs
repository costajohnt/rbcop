//! RuboCop's MultilineExpressionIndentation mixin over a parser-gem-shaped view of the ancestors.

use crate::ast::*;
use crate::cop::Cx;
use crate::node_kind as kind;
use ruby_prism::{CallNode, Node};

/// Parser node types the mixin asks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PK {
    Send,
    Csend,
    /// `block` (incl. lambda literals): `block_type?`
    Block,
    /// numblock / itblock: `any_block_type?` only
    OtherBlock,
    /// `begin` with `loc.begin` (parentheses, `#{}`)
    Begin,
    /// `begin` without `loc.begin` (a multi-statement body)
    ImplicitBegin,
    KwBegin,
    If,
    While,
    Until,
    For,
    Return,
    Array,
    Hash,
    Pair,
    Splat,
    KwSplat,
    Def,
    /// lvasgn ... masgn, op_asgn, or_asgn, and_asgn
    Asgn,
    Other,
}

/// One parser ancestor: its type, the Prism ancestor index it comes from (`SELF` for the block
/// that a call node itself carries) and its parser source range.
#[derive(Clone, Copy, Debug)]
pub struct PAnc {
    pub kind: PK,
    pub idx: usize,
    pub start: usize,
    pub end: usize,
}

pub const SELF: usize = usize::MAX;

fn block_kind(block: &Node) -> PK {
    match block.as_block_node().and_then(|b| b.parameters()).map(|p| kind::of(&p)) {
        Some(kind::NUMBERED_PARAMETERS_NODE | kind::IT_PARAMETERS_NODE) => PK::OtherBlock,
        _ => PK::Block,
    }
}

/// The call's `{}`/`do end` block, if any (not a `&blk` argument).
pub fn block_of<'pr>(call: &CallNode<'pr>) -> Option<ruby_prism::BlockNode<'pr>> {
    call.block().and_then(|b| b.as_block_node())
}

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// Parser-gem ancestors of `node` (whose Prism ancestors are `ancestors`), nearest first.
pub fn pancestors(ancestors: &[Node], node: &Node) -> Vec<PAnc> {
    let mut out = Vec::new();
    if let Some(c) = node.as_call_node()
        && let Some(b) = c.block().filter(|b| b.as_block_node().is_some())
    {
        let (s, e) = span(node);
        out.push(PAnc { kind: block_kind(&b), idx: SELF, start: s, end: e });
    }
    let mut i = ancestors.len();
    while i > 0 {
        i -= 1;
        let a = &ancestors[i];
        let (s, e) = span(a);
        let parent = i.checked_sub(1).map(|j| &ancestors[j]);
        let k = match kind::of(a) {
            kind::PROGRAM_NODE | kind::ARGUMENTS_NODE | kind::ELSE_NODE => continue,
            kind::STATEMENTS_NODE => {
                let body: Vec<Node> = a.as_statements_node().unwrap().body().iter().collect();
                let absorbed = parent.is_some_and(|p| {
                    matches!(kind::of(p), kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE)
                        || p.as_begin_node().is_some_and(|b| {
                            b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none()
                        })
                });
                if body.len() > 1 && !absorbed {
                    let (s, _) = span(&body[0]);
                    let (_, e) = span(body.last().unwrap());
                    out.push(PAnc { kind: PK::ImplicitBegin, idx: i, start: s, end: e });
                }
                continue;
            }
            kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE => PK::Begin,
            kind::BLOCK_NODE => {
                let Some(owner) = parent else { continue };
                out.push(PAnc { kind: block_kind(a), idx: i - 1, start: span(owner).0, end: e });
                i -= 1;
                continue;
            }
            kind::LAMBDA_NODE => PK::Block,
            kind::CALL_NODE => {
                let call = a.as_call_node().unwrap();
                let (ss, se) = send_range(&call);
                let k = if is_csend(&call) { PK::Csend } else { PK::Send };
                out.push(PAnc { kind: k, idx: i, start: ss, end: se });
                if let Some(b) = call.block().filter(|b| b.as_block_node().is_some()) {
                    out.push(PAnc { kind: block_kind(&b), idx: i, start: s, end: e });
                }
                continue;
            }
            kind::SUPER_NODE | kind::FORWARDING_SUPER_NODE => {
                let block = match a.as_super_node() {
                    Some(n) => n.block(),
                    None => a.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
                };
                let block = block.filter(|b| b.as_block_node().is_some());
                let end = block.as_ref().map_or(e, |b| b.location().start_offset());
                out.push(PAnc { kind: PK::Other, idx: i, start: s, end });
                if let Some(b) = block {
                    out.push(PAnc { kind: block_kind(&b), idx: i, start: s, end: e });
                }
                continue;
            }
            kind::IF_NODE | kind::UNLESS_NODE => PK::If,
            kind::WHILE_NODE if !a.as_while_node().unwrap().is_begin_modifier() => PK::While,
            kind::UNTIL_NODE if !a.as_until_node().unwrap().is_begin_modifier() => PK::Until,
            kind::FOR_NODE => PK::For,
            kind::RETURN_NODE => PK::Return,
            kind::ARRAY_NODE => PK::Array,
            kind::HASH_NODE | kind::KEYWORD_HASH_NODE => PK::Hash,
            kind::ASSOC_NODE => PK::Pair,
            kind::SPLAT_NODE => PK::Splat,
            kind::ASSOC_SPLAT_NODE => PK::KwSplat,
            kind::DEF_NODE => PK::Def,
            kind::BEGIN_NODE if a.as_begin_node().unwrap().begin_keyword_loc().is_some() => PK::KwBegin,
            _ if assignment_parts(a).is_some() => PK::Asgn,
            _ => PK::Other,
        };
        out.push(PAnc { kind: k, idx: i, start: s, end: e });
    }
    out
}

pub fn within(inner: (usize, usize), outer: (usize, usize)) -> bool {
    inner.0 >= outer.0 && inner.1 <= outer.1
}

/// Everything a mixin method needs about the node under inspection.
pub struct Ctx<'a, 'pr> {
    pub ancestors: &'a [Node<'pr>],
    pub node: &'a Node<'pr>,
    /// Parser source range of the node.
    pub range: (usize, usize),
    pub anc: Vec<PAnc>,
}

impl<'a, 'pr> Ctx<'a, 'pr> {
    pub fn new(ancestors: &'a [Node<'pr>], node: &'a Node<'pr>, range: (usize, usize)) -> Self {
        Ctx { ancestors, node, range, anc: pancestors(ancestors, node) }
    }

    pub fn prism(&self, a: &PAnc) -> &'a Node<'pr> {
        if a.idx == SELF { self.node } else { &self.ancestors[a.idx] }
    }

    pub fn call(&self, a: &PAnc) -> Option<CallNode<'pr>> {
        self.prism(a).as_call_node()
    }

    fn is_ternary(&self, a: &PAnc) -> bool {
        self.prism(a).as_if_node().is_some_and(|n| n.if_keyword_loc().is_none())
    }

    /// `indented_keyword_expression`: the condition, `for` collection or first returned value.
    pub fn keyword_expression(&self, a: &PAnc) -> Option<(usize, usize)> {
        let n = self.prism(a);
        let e = match a.kind {
            PK::If => match n.as_if_node() {
                Some(i) => i.predicate(),
                None => n.as_unless_node()?.predicate(),
            },
            PK::While => n.as_while_node()?.predicate(),
            PK::Until => n.as_until_node()?.predicate(),
            PK::For => n.as_for_node()?.collection(),
            PK::Return => n.as_return_node()?.arguments()?.arguments().iter().next()?,
            _ => return None,
        };
        Some(span(&e))
    }

    /// kw_node_with_special_indentation
    pub fn keyword_node(&self) -> Option<&PAnc> {
        self.anc.iter().find(|a| {
            matches!(a.kind, PK::If | PK::While | PK::Until | PK::For | PK::Return)
                && !self.is_ternary(a)
                && self.keyword_expression(a).is_some_and(|k| within(self.range, k))
        })
    }

    fn keyword_text(&self, a: &PAnc) -> &'pr str {
        let n = self.prism(a);
        let loc = match kind::of(n) {
            kind::IF_NODE => n.as_if_node().unwrap().if_keyword_loc(),
            kind::UNLESS_NODE => Some(n.as_unless_node().unwrap().keyword_loc()),
            kind::WHILE_NODE => Some(n.as_while_node().unwrap().keyword_loc()),
            kind::UNTIL_NODE => Some(n.as_until_node().unwrap().keyword_loc()),
            kind::FOR_NODE => Some(n.as_for_node().unwrap().for_keyword_loc()),
            kind::RETURN_NODE => Some(n.as_return_node().unwrap().keyword_loc()),
            _ => None,
        };
        loc.map_or("", |l| text(&l))
    }

    /// postfix_conditional?: a modifier `if`/`unless`.
    fn postfix_conditional(&self, a: &PAnc) -> bool {
        let n = self.prism(a);
        match kind::of(n) {
            kind::IF_NODE => {
                let i = n.as_if_node().unwrap();
                i.end_keyword_loc().is_none() && i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if")
            }
            kind::UNLESS_NODE => n.as_unless_node().unwrap().end_keyword_loc().is_none(),
            _ => false,
        }
    }

    /// correct_indentation
    pub fn correct_indentation(&self, cx: &Cx, width: i64) -> i64 {
        match self.keyword_node() {
            Some(kw) if !self.postfix_conditional(kw) => {
                width + cx.config.for_cop("Layout/IndentationWidth").int("Width").unwrap_or(2)
            }
            _ => width,
        }
    }

    /// Parser `arguments` of a send ancestor (block_pass included).
    pub fn args(&self, a: &PAnc) -> Vec<Node<'pr>> {
        self.call(a).map(|c| send_args(&c)).unwrap_or_default()
    }

    fn setter(&self, a: &PAnc) -> bool {
        self.call(a).is_some_and(|c| c.equal_loc().is_some())
    }

    pub fn parenthesized(&self, a: &PAnc) -> bool {
        self.call(a).and_then(|c| c.closing_loc()).is_some_and(|l| l.as_slice() == b")")
    }

    /// argument_in_method_call: the nearest send whose arguments hold the node, stopping at a block.
    pub fn argument_in_method_call(&self, with_parentheses: bool) -> Option<&PAnc> {
        for a in &self.anc {
            match a.kind {
                PK::Block => return None,
                PK::Send => {
                    if self.setter(a) || (with_parentheses && !self.parenthesized(a)) {
                        continue;
                    }
                    if self.args(a).iter().any(|arg| within(self.range, span(arg))) {
                        return Some(a);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// def_modifier? of a send ancestor: `private def x` (possibly nested modifiers).
    pub fn def_modifier(&self, a: &PAnc) -> bool {
        fn check(call: &CallNode) -> bool {
            if is_csend(call) || call.receiver().is_some() {
                return false;
            }
            match send_args(call).into_iter().next() {
                Some(arg) if kind::of(&arg) == kind::DEF_NODE => true,
                Some(arg) => arg.as_call_node().filter(|c| block_of(c).is_none()).is_some_and(|c| check(&c)),
                None => false,
            }
        }
        self.call(a).is_some_and(|c| check(&c))
    }

    /// assignment_rhs: the value of an assignment, the last argument of a setter send.
    pub fn assignment_rhs(&self, a: &PAnc) -> Option<(usize, usize)> {
        match a.kind {
            PK::Asgn => assignment_parts(self.prism(a)).map(|(_, v)| span(&v)),
            PK::Send | PK::Csend => self.args(a).last().map(span),
            _ => None,
        }
    }

    fn block_body(&self, a: &PAnc) -> Option<(usize, usize)> {
        let n = self.prism(a);
        let body = match n.as_lambda_node() {
            Some(l) => l.body(),
            None => block_of(&n.as_call_node()?)?.body(),
        };
        body.map(|b| span(&b))
    }

    /// part_of_assignment_rhs
    pub fn part_of_assignment_rhs(&self, candidate: Option<(usize, usize)>) -> Option<&PAnc> {
        for a in &self.anc {
            let disqualified = matches!(a.kind, PK::If | PK::While | PK::Until | PK::For | PK::Return | PK::Array | PK::KwBegin)
                || (a.kind == PK::Block && self.block_body(a).is_some_and(|b| candidate.is_none_or(|c| within(c, b))));
            if disqualified {
                return None;
            }
            let valid = match a.kind {
                PK::Send => self.setter(a),
                PK::Csend => self.setter(a),
                PK::Asgn => true,
                _ => false,
            } && self.assignment_rhs(a).is_some_and(|r| candidate.is_none_or(|c| within(c, r)));
            if valid {
                return Some(a);
            }
        }
        None
    }

    /// not_for_this_cop?
    pub fn not_for_this_cop(&self) -> bool {
        self.anc.iter().any(|a| a.kind == PK::Begin || self.inside_arg_list_parentheses(a))
    }

    pub fn inside_arg_list_parentheses(&self, a: &PAnc) -> bool {
        if a.kind != PK::Send || !self.parenthesized(a) {
            return false;
        }
        let call = self.call(a).unwrap();
        let (Some(open), Some(close)) = (call.opening_loc(), call.closing_loc()) else { return false };
        self.range.0 > open.start_offset() && self.range.1 < close.end_offset()
    }

    /// operation_description
    pub fn operation_description(&self, rhs: Option<(usize, usize)>) -> String {
        if let Some(kw) = self.keyword_node() {
            let keyword = self.keyword_text(kw);
            let what = if keyword == "for" { "collection" } else { "condition" };
            let article = if keyword.starts_with(['i', 'u']) { "an" } else { "a" };
            return format!("a {what} in {article} `{keyword}` statement");
        }
        if self.part_of_assignment_rhs(rhs).is_some() {
            return "an expression in an assignment".into();
        }
        "an expression".into()
    }
}

/// `node.source_range.source_line =~ /\S/` for a node starting at `start`.
pub fn indentation(cx: &Cx, start: usize) -> i64 {
    crate::helpers::alignment_check::first_non_space(cx.src.lines[cx.src.line_of(start) - 1]) as i64
}
