use crate::prelude::*;
use crate::source::Source;
use helpers::node_equality::same;
use helpers::parentheses_corrector::{self, ParenFix};
use helpers::parser_parent::parser_parent_index;
use helpers::reparsed_equivalence::{parses_equivalently, serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub const NAME: &str = "Style/RedundantParentheses";
pub static COP: RedundantParentheses = RedundantParentheses;
pub struct RedundantParentheses;

const MAX_VERIFICATION_FRAGMENT_SIZE: usize = 64 * 1024;

struct Pending {
    range: (usize, usize),
    msg: String,
    fix: ParenFix,
    /// reparse_scope: the innermost def/class/module/sclass around the offense (None: the file).
    scope: Option<(usize, usize)>,
}

#[derive(Default)]
struct State {
    pending: Vec<Pending>,
    /// Normalized structure of each scope's original code.
    scopes: HashMap<Option<(usize, usize)>, String>,
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// Children of a parser `begin` (parentheses, a statement list, an interpolation).
fn begin_children<'pr>(n: &Node<'pr>) -> Vec<Node<'pr>> {
    let body = match kind::of(n) {
        kind::PARENTHESES_NODE => n.as_parentheses_node().unwrap().body(),
        kind::EMBEDDED_STATEMENTS_NODE => {
            return n
                .as_embedded_statements_node()
                .unwrap()
                .statements()
                .map(|s| s.body().iter().collect())
                .unwrap_or_default();
        }
        kind::STATEMENTS_NODE => return n.as_statements_node().unwrap().body().iter().collect(),
        _ => return vec![],
    };
    match body {
        None => vec![],
        Some(b) => match b.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![b],
        },
    }
}

/// Parser `begin_type?` of the parser parent `anc[i]`.
fn is_begin(anc: &[Node], i: usize) -> bool {
    match kind::of(&anc[i]) {
        kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE => true,
        // A statement list directly in `begin..end` without rescue/ensure is parser's kwbegin.
        kind::STATEMENTS_NODE => !(i > 0
            && anc[i - 1].as_begin_node().is_some_and(|b| {
                b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none()
            })),
        _ => false,
    }
}

/// Number of parser children of the parser parent `anc[i]` (only exact where it can be one).
fn children_count(anc: &[Node], i: usize) -> usize {
    let n = &anc[i];
    let args = |a: Option<ruby_prism::ArgumentsNode>| a.map_or(0, |a| a.arguments().iter().count());
    match kind::of(n) {
        kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE | kind::STATEMENTS_NODE => begin_children(n).len(),
        kind::ARRAY_NODE => n.as_array_node().unwrap().elements().iter().count(),
        kind::RETURN_NODE => args(n.as_return_node().unwrap().arguments()),
        kind::NEXT_NODE => args(n.as_next_node().unwrap().arguments()),
        kind::BREAK_NODE => args(n.as_break_node().unwrap().arguments()),
        kind::YIELD_NODE => args(n.as_yield_node().unwrap().arguments()),
        kind::SUPER_NODE => {
            let s = n.as_super_node().unwrap();
            args(s.arguments()) + usize::from(s.block().is_some_and(|b| b.as_block_argument_node().is_some()))
        }
        kind::DEFINED_NODE | kind::BLOCK_ARGUMENT_NODE | kind::SPLAT_NODE | kind::ASSOC_SPLAT_NODE => 1,
        kind::BEGIN_NODE => {
            let b = n.as_begin_node().unwrap();
            if b.rescue_clause().is_some() {
                3
            } else if b.ensure_clause().is_some() {
                2
            } else {
                b.statements().map_or(0, |s| s.body().iter().count())
            }
        }
        // `children.one?` counts non-nil children: an empty body or else is nil.
        kind::IF_NODE if !is_ternary(n) => {
            let i = n.as_if_node().unwrap();
            let els = i.subsequent().is_some_and(|s| s.as_else_node().is_none_or(|e| non_empty(e.statements())));
            1 + usize::from(non_empty(i.statements())) + usize::from(els)
        }
        kind::UNLESS_NODE => {
            let u = n.as_unless_node().unwrap();
            let els = u.else_clause().is_some_and(|e| non_empty(e.statements()));
            1 + usize::from(non_empty(u.statements())) + usize::from(els)
        }
        kind::WHILE_NODE => 1 + usize::from(non_empty(n.as_while_node().unwrap().statements())),
        kind::UNTIL_NODE => 1 + usize::from(non_empty(n.as_until_node().unwrap().statements())),
        kind::WHEN_NODE => {
            let w = n.as_when_node().unwrap();
            w.conditions().iter().count() + usize::from(non_empty(w.statements()))
        }
        kind::CASE_NODE => {
            let c = n.as_case_node().unwrap();
            usize::from(c.predicate().is_some())
                + c.conditions().iter().count()
                + usize::from(c.else_clause().is_some_and(|e| non_empty(e.statements())))
        }
        _ => 2,
    }
}

fn non_empty(s: Option<ruby_prism::StatementsNode>) -> bool {
    s.is_some_and(|s| s.body().iter().next().is_some())
}

/// rubocop-ast `keyword?` of a parser parent.
fn is_keyword(n: &Node) -> bool {
    match kind::of(n) {
        kind::AND_NODE => n.as_and_node().unwrap().operator_loc().as_slice() == b"and",
        kind::OR_NODE => n.as_or_node().unwrap().operator_loc().as_slice() == b"or",
        kind::CALL_NODE => prefix_not(n),
        kind::BEGIN_NODE => true,
        kind::ALIAS_METHOD_NODE
        | kind::ALIAS_GLOBAL_VARIABLE_NODE
        | kind::BREAK_NODE
        | kind::CASE_NODE
        | kind::CLASS_NODE
        | kind::DEF_NODE
        | kind::DEFINED_NODE
        | kind::FOR_NODE
        | kind::IF_NODE
        | kind::UNLESS_NODE
        | kind::MODULE_NODE
        | kind::NEXT_NODE
        | kind::POST_EXECUTION_NODE
        | kind::REDO_NODE
        | kind::RESCUE_MODIFIER_NODE
        | kind::RETRY_NODE
        | kind::RETURN_NODE
        | kind::SELF_NODE
        | kind::SUPER_NODE
        | kind::FORWARDING_SUPER_NODE
        | kind::UNDEF_NODE
        | kind::UNTIL_NODE
        | kind::WHEN_NODE
        | kind::WHILE_NODE
        | kind::YIELD_NODE => true,
        _ => false,
    }
}

fn prefix_not(n: &Node) -> bool {
    n.as_call_node().is_some_and(|c| method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"not"))
}

fn is_send(n: &Node) -> bool {
    n.as_call_node().is_some_and(|c| !is_csend(&c))
}

fn is_ternary(n: &Node) -> bool {
    n.as_if_node().is_some_and(|i| i.if_keyword_loc().is_none())
}

fn is_variable(n: &Node) -> bool {
    matches!(
        kind::of(n),
        kind::LOCAL_VARIABLE_READ_NODE
            | kind::IT_LOCAL_VARIABLE_READ_NODE
            | kind::INSTANCE_VARIABLE_READ_NODE
            | kind::CLASS_VARIABLE_READ_NODE
            | kind::GLOBAL_VARIABLE_READ_NODE
    )
}

/// `call_type?` of a node that is not a parser `block` (send/csend).
fn call_type(n: &Node) -> bool {
    n.as_call_node().is_some_and(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// Parser arguments of a send/super/yield (a block pass included).
fn arguments<'pr>(n: &Node<'pr>) -> Vec<Node<'pr>> {
    let (args, block) = match kind::of(n) {
        kind::CALL_NODE => {
            let c = n.as_call_node().unwrap();
            (c.arguments(), c.block())
        }
        kind::SUPER_NODE => {
            let s = n.as_super_node().unwrap();
            (s.arguments(), s.block())
        }
        kind::YIELD_NODE => (n.as_yield_node().unwrap().arguments(), None),
        kind::DEFINED_NODE => return vec![n.as_defined_node().unwrap().value()],
        _ => return vec![],
    };
    let mut v: Vec<Node> = args.map(|a| a.arguments().iter().collect()).unwrap_or_default();
    v.extend(block.filter(|b| b.as_block_argument_node().is_some()));
    v
}

/// `parenthesized?` / `parentheses?(node)`: the call's own `(`...`)`.
fn parenthesized(n: &Node) -> bool {
    match kind::of(n) {
        kind::CALL_NODE => n.as_call_node().unwrap().closing_loc().is_some_and(|c| c.as_slice() == b")"),
        kind::SUPER_NODE => n.as_super_node().unwrap().rparen_loc().is_some(),
        kind::YIELD_NODE => n.as_yield_node().unwrap().rparen_loc().is_some(),
        kind::DEFINED_NODE => n.as_defined_node().unwrap().rparen_loc().is_some(),
        _ => false,
    }
}

fn lambda_or_proc(n: &Node) -> bool {
    if n.as_lambda_node().is_some() {
        return true;
    }
    let Some(c) = n.as_call_node() else { return false };
    if c.block().is_none_or(|b| b.as_block_node().is_none()) || is_csend(&c) {
        return false;
    }
    let name = method_name(&c);
    match c.receiver() {
        None => name == "lambda" || name == "proc",
        Some(r) => name == "new" && helpers::parser_parent::is_top_const(&r, "Proc"),
    }
}

/// `braces?` of a block.
fn braces(n: &Node) -> bool {
    match kind::of(n) {
        kind::LAMBDA_NODE => n.as_lambda_node().unwrap().opening_loc().as_slice() == b"{",
        _ => n
            .as_call_node()
            .and_then(|c| c.block())
            .and_then(|b| b.as_block_node())
            .is_some_and(|b| b.opening_loc().as_slice() == b"{"),
    }
}

fn is_block(n: &Node) -> bool {
    n.as_lambda_node().is_some()
        || n.as_call_node().and_then(|c| c.block()).is_some_and(|b| b.as_block_node().is_some())
        || n.as_super_node().and_then(|c| c.block()).is_some_and(|b| b.as_block_node().is_some())
        || n.as_forwarding_super_node().is_some_and(|c| c.block().is_some())
}

/// keyword_with_redundant_parentheses?
fn keyword_with_redundant_parentheses(n: &Node) -> bool {
    let only_paren = |args: &[Node]| args.len() == 1 && args[0].as_parentheses_node().is_some();
    match kind::of(n) {
        kind::SOURCE_FILE_NODE | kind::SOURCE_LINE_NODE | kind::SOURCE_ENCODING_NODE => true,
        kind::SELF_NODE | kind::REDO_NODE | kind::RETRY_NODE => true,
        kind::FORWARDING_SUPER_NODE => n.as_forwarding_super_node().unwrap().block().is_none(),
        kind::RETURN_NODE | kind::NEXT_NODE | kind::BREAK_NODE => {
            let args: Vec<Node> = match kind::of(n) {
                kind::RETURN_NODE => n.as_return_node().unwrap().arguments(),
                kind::NEXT_NODE => n.as_next_node().unwrap().arguments(),
                _ => n.as_break_node().unwrap().arguments(),
            }
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
            args.is_empty() || only_paren(&args)
        }
        kind::YIELD_NODE | kind::SUPER_NODE | kind::DEFINED_NODE => {
            if is_block(n) {
                return false;
            }
            let args = arguments(n);
            if only_paren(&args) { true } else { args.is_empty() || parenthesized(n) }
        }
        kind::BEGIN_NODE => {
            let b = n.as_begin_node().unwrap();
            if b.begin_keyword_loc().is_none() || b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                return false;
            }
            let stmts: Vec<Node> = b.statements().map(|s| s.body().iter().collect()).unwrap_or_default();
            stmts.is_empty() || only_paren(&stmts)
        }
        _ => false,
    }
}

/// `square_brackets?`: `(send `{(send _recv _msg) str array hash const #variable?} :[] ...)`.
fn square_brackets(n: &Node) -> bool {
    let Some(c) = n.as_call_node().filter(|c| !is_csend(c) && method_name(c) == "[]") else { return false };
    let Some(recv) = c.receiver() else { return false };
    struct Find(bool);
    impl<'pr> ruby_prism::Visit<'pr> for Find {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            self.check(&node);
        }
        fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
            self.check(&node);
        }
    }
    impl Find {
        fn check(&mut self, n: &Node) {
            self.0 |= match kind::of(n) {
                kind::CALL_NODE => {
                    let c = n.as_call_node().unwrap();
                    !is_csend(&c) && c.arguments().is_none() && c.block().is_none_or(|b| b.as_block_node().is_some())
                }
                kind::STRING_NODE => !is_parser_dstr(n),
                kind::ARRAY_NODE | kind::HASH_NODE | kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE => true,
                _ => is_variable(n),
            };
        }
    }
    let mut f = Find(false);
    ruby_prism::Visit::visit(&mut f, &recv);
    f.0
}

/// `unary_operation?`
fn unary_operation(n: &Node) -> bool {
    n.as_call_node().is_some_and(|c| {
        c.message_loc().is_some_and(|m| m.start_offset() == n.location().start_offset())
            && helpers::node_types::is_operator_method(method_name(&c))
    })
}

struct Check<'a, 'pr> {
    src: &'a Source<'a>,
    anc: &'a [Node<'pr>],
    node: &'a Node<'pr>,
    /// Index of the parser parent in `anc`.
    pi: Option<usize>,
}

impl<'pr> Check<'_, 'pr> {
    fn parent(&self) -> Option<&Node<'pr>> {
        self.pi.map(|i| &self.anc[i])
    }

    fn me(&self) -> (usize, usize) {
        range(self.node)
    }

    fn parent_is(&self, f: impl Fn(&Node) -> bool) -> bool {
        self.parent().is_some_and(f)
    }

    fn parent_begin(&self) -> bool {
        self.pi.is_some_and(|i| is_begin(self.anc, i))
    }

    /// `begin_node.chained?`
    fn chained(&self) -> bool {
        self.parent()
            .and_then(|p| p.as_call_node())
            .and_then(|c| c.receiver())
            .is_some_and(|r| same(self.src, range(&r), self.me()))
    }

    fn rescue(&self) -> bool {
        let n = self.anc.len();
        match self.anc.last().map(kind::of) {
            Some(kind::RESCUE_NODE) => true,
            Some(kind::STATEMENTS_NODE) => n >= 2 && self.anc[n - 2].as_rescue_node().is_some(),
            Some(kind::RESCUE_MODIFIER_NODE) => {
                let r = self.anc[n - 1].as_rescue_modifier_node().unwrap();
                range(&r.rescue_expression()) == self.me()
            }
            _ => false,
        }
    }

    fn parens_required(&self) -> bool {
        let (s, e) = self.me();
        let b = self.src.text.as_bytes();
        // Ruby's `source[begin_pos - 1]` wraps to the last character at offset 0.
        let before = if s == 0 { b.last().copied() } else { Some(b[s - 1]) };
        before.is_some_and(|c| c.is_ascii_lowercase()) || b.get(e).is_some_and(|c| c.is_ascii_lowercase())
    }

    fn parens_allowed(&self, cx: &Cx, kids: &[Node]) -> bool {
        if kids.is_empty() || self.rescue() {
            return true;
        }
        // in_pattern_matching_in_method_argument?
        if self.parent_is(call_type) {
            let pattern = if cx.config.target_ruby <= 2.7 {
                kids[0].as_match_required_node().is_some()
            } else {
                kids[0].as_match_predicate_node().is_some()
            };
            if pattern {
                return true;
            }
        }
        // allowed_expression?
        if self.parent_is(is_keyword) && self.parens_required() {
            return true;
        }
        if kids.len() != 1 && self.parent().is_some_and(|p| !is_begin_like(self.anc, self.pi.unwrap(), p)) {
            return true;
        }
        if self.parent_is(is_ternary) {
            let t = cx.config.for_cop("Style/TernaryParentheses");
            if t.truthy("Enabled")
                && matches!(t.str("EnforcedStyle"), Some("require_parentheses" | "require_parentheses_when_complex"))
            {
                return true;
            }
        }
        self.parent_is(|p| p.as_range_node().is_some())
    }

    fn ignore_syntax(&self) -> bool {
        let Some(p) = self.parent() else { return false };
        let post = p.as_while_node().is_some_and(|w| w.is_begin_modifier())
            || p.as_until_node().is_some_and(|w| w.is_begin_modifier());
        let match_lvasgn = p.as_call_node().is_some_and(|c| {
            method_name(&c) == "=~" && c.receiver().is_some_and(|r| r.as_regular_expression_node().is_some())
        });
        if post || match_lvasgn {
            return true;
        }
        // like_method_argument_parentheses?
        if matches!(kind::of(p), kind::CALL_NODE | kind::SUPER_NODE | kind::YIELD_NODE) && !is_csend_node(p) {
            let args = arguments(p);
            let op = p.as_call_node().is_some_and(|c| helpers::node_types::is_operator_method(method_name(&c)));
            if args.len() == 1 && !parenthesized(p) && !op && args[0].as_parentheses_node().is_some() {
                return true;
            }
        }
        // multiline_control_flow_statements?
        let (ps, pe) = range(p);
        matches!(kind::of(p), kind::RETURN_NODE | kind::NEXT_NODE | kind::BREAK_NODE)
            && self.src.line_of(ps) != self.src.line_of(pe)
    }

    fn argument_of_parenthesized_method_call(&self, node: &Node) -> bool {
        let conditional = matches!(
            kind::of(node),
            kind::IF_NODE | kind::UNLESS_NODE | kind::WHILE_NODE | kind::UNTIL_NODE | kind::RESCUE_MODIFIER_NODE
        );
        let required = call_type(node) && {
            let c = node.as_call_node().unwrap();
            (c.receiver().is_none() || c.call_operator_loc().is_some()) && !arguments(node).is_empty()
        };
        if conditional || required {
            return false;
        }
        self.parent().is_some_and(|p| {
            call_type(p)
                && parenthesized(p)
                && p.as_call_node().unwrap().receiver().is_none_or(|r| !same(self.src, range(&r), self.me()))
        })
    }

    fn find_offense_message(&self, cx: &Cx, node: &Node) -> Option<&'static str> {
        if keyword_with_redundant_parentheses(node) {
            return Some("a keyword");
        }
        if helpers::literal_node::literal(node) && self.disallowed_literal(node) {
            return Some("a literal");
        }
        if is_variable(node) {
            return Some("a variable");
        }
        if matches!(kind::of(node), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE) {
            return Some("a constant");
        }
        if self.parent_is(|p| matches!(kind::of(p), kind::BLOCK_NODE | kind::LAMBDA_NODE)) || self.body_range(node) {
            return Some("block body");
        }
        if is_assignment(node) && (self.parent().is_none() || self.parent_begin()) {
            return Some("an assignment");
        }
        if lambda_or_proc(node) && (braces(node) || node.as_lambda_node().is_some()) {
            return Some("an expression");
        }
        if self.disallowed_one_line_pattern_matching(node) {
            return Some("a one-line pattern matching");
        }
        if self.interpolation() {
            return Some("an interpolated expression");
        }
        if self.argument_of_parenthesized_method_call(node) && !keyword_logical_operator(node) {
            return Some("a method argument");
        }
        if self.oneline_rescue_parentheses_required(node) {
            return Some("a one-line rescue");
        }
        if self.chained() {
            return None;
        }
        if matches!(kind::of(node), kind::AND_NODE | kind::OR_NODE) {
            let parent = self.parent();
            if keyword_logical_operator(node) && parent.is_some() {
                return None;
            }
            let (s, e) = range(node);
            if self.src.line_of(s) != self.src.line_of(e) && allow_in_multiline_conditions(cx) {
                return None;
            }
            if parent.is_some_and(|p| {
                matches!(kind::of(p), kind::OR_NODE | kind::SPLAT_NODE | kind::ASSOC_SPLAT_NODE)
                    || (call_type(p) && is_send(p))
            }) {
                return None;
            }
            if node.as_or_node().is_some() && self.parent_is(|p| p.as_and_node().is_some()) {
                return None;
            }
            if self.parent_is(is_ternary) {
                return None;
            }
            return Some("a logical expression");
        }
        let comparison = node.as_call_node().is_some_and(|c| {
            c.block().is_none_or(|b| b.as_block_node().is_none())
                && matches!(method_name(&c), "==" | "===" | "!=" | "<=" | ">=" | ">" | "<")
        });
        if comparison && self.parent().is_none() {
            return Some("a comparison expression");
        }
        None
    }

    fn disallowed_literal(&self, node: &Node) -> bool {
        if node.as_range_node().is_none() {
            return true;
        }
        self.pi.is_some_and(|i| is_begin(self.anc, i) && children_count(self.anc, i) == 1)
    }

    fn body_range(&self, node: &Node) -> bool {
        if self.chained() {
            return false;
        }
        let Some(r) = node.as_range_node() else { return false };
        let Some(i) = self.pi.filter(|&i| is_begin(self.anc, i)) else { return false };
        let kids = begin_children(&self.anc[i]);
        let (first, last) = (kids.first().map(range), kids.last().map(range));
        (r.left().is_none() && first.is_some_and(|f| same(self.src, f, self.me())))
            || (r.right().is_none() && last.is_some_and(|l| same(self.src, l, self.me())))
    }

    fn disallowed_one_line_pattern_matching(&self, node: &Node) -> bool {
        if let Some(p) = self.parent() {
            if p.as_def_node().is_some_and(|d| d.equal_loc().is_some()) {
                return false;
            }
            if is_assignment(p) {
                return false;
            }
        }
        matches!(kind::of(node), kind::MATCH_PREDICATE_NODE | kind::MATCH_REQUIRED_NODE)
            && !self.anc.iter().any(|a| matches!(kind::of(a), kind::AND_NODE | kind::OR_NODE))
    }

    /// `[^begin ^^dstr]`
    fn interpolation(&self) -> bool {
        let mut i = self.anc.len();
        while i > 0 && kind::of(&self.anc[i - 1]) == kind::STATEMENTS_NODE {
            i -= 1;
        }
        i >= 2
            && kind::of(&self.anc[i - 1]) == kind::EMBEDDED_STATEMENTS_NODE
            && kind::of(&self.anc[i - 2]) == kind::INTERPOLATED_STRING_NODE
    }

    fn oneline_rescue_parentheses_required(&self, node: &Node) -> bool {
        if node.as_rescue_modifier_node().is_none() {
            return false;
        }
        let Some(p) = self.parent() else { return false };
        if is_ternary(p) {
            return false;
        }
        let condition = match kind::of(p) {
            kind::IF_NODE => Some(p.as_if_node().unwrap().predicate()),
            kind::UNLESS_NODE => Some(p.as_unless_node().unwrap().predicate()),
            kind::WHILE_NODE => Some(p.as_while_node().unwrap().predicate()),
            kind::UNTIL_NODE => Some(p.as_until_node().unwrap().predicate()),
            kind::CASE_NODE => p.as_case_node().unwrap().predicate(),
            kind::CASE_MATCH_NODE => p.as_case_match_node().unwrap().predicate(),
            _ => None,
        };
        if condition.is_some_and(|c| same(self.src, range(&c), self.me())) {
            return false;
        }
        !(call_type(p) || matches!(kind::of(p), kind::ARRAY_NODE | kind::ASSOC_NODE))
    }

    /// check_send / check_unary: the message, if any.
    fn check_send(&self, node: &Node) -> Option<&'static str> {
        let call_node = call_type(node) || (is_block(node) && braces(node) && !lambda_or_proc(node));
        if !call_node {
            return None;
        }
        // `node.send_node` of a block is the same Prism call; its arguments exclude the block.
        if unary_operation(node) {
            if self.chained() {
                return None;
            }
            let mut n = node.as_call_node().unwrap().as_node();
            while is_send(&n) && unary_operation(&n) && !prefix_not(&n) {
                match n.as_call_node().unwrap().receiver() {
                    Some(r) => n = r,
                    None => break,
                }
            }
            return self.method_call_with_redundant_parentheses(&n).then_some("a unary operation");
        }
        self.method_call_with_redundant_parentheses(node).then_some("a method call")
    }

    fn method_call_with_redundant_parentheses(&self, node: &Node) -> bool {
        let ok = matches!(kind::of(node), kind::CALL_NODE | kind::YIELD_NODE | kind::DEFINED_NODE | kind::SUPER_NODE);
        if !ok || prefix_not(node) {
            return false;
        }
        // singular_parenthesized_parent?
        match self.pi {
            None => return true,
            Some(i) => {
                let p = &self.anc[i];
                if !matches!(kind::of(p), kind::SPLAT_NODE | kind::ASSOC_SPLAT_NODE) && children_count(self.anc, i) == 1 {
                    return true;
                }
            }
        }
        arguments(node).is_empty() || parenthesized(node) || square_brackets(node)
    }
}

/// `begin`, `any_def` or `any_block` parent (allowed_multiple_expression?).
fn is_begin_like(anc: &[Node], i: usize, p: &Node) -> bool {
    is_begin(anc, i) || matches!(kind::of(p), kind::DEF_NODE | kind::BLOCK_NODE | kind::LAMBDA_NODE)
}

fn is_csend_node(n: &Node) -> bool {
    n.as_call_node().is_some_and(|c| is_csend(&c))
}

fn keyword_logical_operator(n: &Node) -> bool {
    match kind::of(n) {
        kind::AND_NODE => n.as_and_node().unwrap().operator_loc().as_slice() == b"and",
        kind::OR_NODE => n.as_or_node().unwrap().operator_loc().as_slice() == b"or",
        _ => false,
    }
}

fn allow_in_multiline_conditions(cx: &Cx) -> bool {
    let c = cx.config.for_cop("Style/ParenthesesAroundCondition");
    c.truthy("Enabled") && c.truthy("AllowInMultilineConditions")
}

/// ParenthesesCorrector plan for the parser begin `target` (parentheses or `#{}`), whose parser
/// parent is `anc[parent]`.
fn plan(src: &Source, target: &Node, anc: &[Node], parent: Option<usize>) -> Option<ParenFix> {
    let (open, close) = match target.as_parentheses_node() {
        Some(p) => (p.opening_loc(), p.closing_loc()),
        None => {
            let e = target.as_embedded_statements_node()?;
            (e.opening_loc(), e.closing_loc())
        }
    };
    let kids = begin_children(target);
    let last = kids.last().map(range);
    let heredoc = kids.last().filter(|n| is_heredoc(n)).map(range);
    let question = parent
        .and_then(|i| anc[i].as_if_node())
        .filter(|i| i.if_keyword_loc().is_none())
        .and_then(|i| i.then_keyword_loc().map(|q| q.start_offset()));
    Some(parentheses_corrector::plan(
        src,
        (open.start_offset(), open.end_offset()),
        (close.start_offset(), close.end_offset()),
        last,
        heredoc,
        question,
    ))
}

/// `offense(node, msg)`: record a pending offense (verified by reparse at the end of the file).
fn push_pending(cx: &mut Cx, anc: &[Node], r: (usize, usize), msg: &str, fix: ParenFix) {
    let src = cx.src;
    // reparse_scope: the innermost def/class/module/sclass containing the offense.
    let scope_i = anc.iter().rposition(|a| {
        matches!(kind::of(a), kind::DEF_NODE | kind::CLASS_NODE | kind::MODULE_NODE | kind::SINGLETON_CLASS_NODE)
            && range(a).0 <= r.0
            && r.1 <= range(a).1
    });
    let scope = scope_i.map(|i| range(&anc[i]));
    let state = cx.state::<State>();
    state.scopes.entry(scope).or_insert_with(|| serialize(src.text, &anc[scope_i.unwrap_or(0)]));
    let msg = format!("Don't use parentheses around {msg}.");
    if let Some(p) = state.pending.iter_mut().find(|p| p.range == r) {
        p.msg = msg;
    } else {
        state.pending.push(Pending { range: r, msg, fix, scope });
    }
}

impl Cop for RedundantParentheses {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PARENTHESES_NODE, kind::PINNED_EXPRESSION_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let anc = cx.ancestors;
        let me = range(node);
        if let Some(pin) = node.as_pinned_expression_node() {
            // Parser: `(pin (begin (lvar a)))`; only a variable is not an allowed_pin_operator?.
            let e = pin.expression();
            if !is_variable(&e) {
                return;
            }
            let (o, c) = (pin.lparen_loc(), pin.rparen_loc());
            let fix = parentheses_corrector::plan(
                cx.src,
                (o.start_offset(), o.end_offset()),
                (c.start_offset(), c.end_offset()),
                Some(range(&e)),
                None,
                None,
            );
            let r = (o.start_offset(), c.end_offset());
            push_pending(cx, anc, r, "a variable", fix);
            return;
        }
        // `def (expr).foo`: parser has no begin there.
        if anc.last().and_then(|a| a.as_def_node()).and_then(|d| d.receiver()).is_some_and(|r| range(&r) == me) {
            return;
        }
        let src = cx.src;
        let pi = parser_parent_index(anc, anc.len());
        let ck = Check { src, anc, node, pi };
        let kids = begin_children(node);
        if ck.parens_allowed(cx, &kids) || ck.ignore_syntax() {
            return;
        }
        let first = &kids[0];
        let (target, target_parent, msg) = if let Some(msg) = ck.find_offense_message(cx, first) {
            if msg != "block body"
                && first.as_range_node().is_some()
                && !ck.argument_of_parenthesized_method_call(first)
            {
                // The offense moves to the parent begin.
                let Some(i) = pi else { return };
                (i, parser_parent_index(anc, i), msg)
            } else {
                (usize::MAX, pi, msg)
            }
        } else {
            match ck.check_send(first) {
                Some(msg) => (usize::MAX, pi, msg),
                None => return,
            }
        };
        let target_node = if target == usize::MAX { node } else { &anc[target] };
        let Some(fix) = plan(src, target_node, anc, target_parent) else { return };
        push_pending(cx, anc, range(target_node), msg, fix);
    }

    fn on_file(&self, cx: &mut Cx) {
        let state = std::mem::take(cx.state::<State>());
        if state.pending.is_empty() {
            return;
        }
        let src = cx.src;
        let source: Arc<str> = Arc::from(src.text);
        let mut order: Vec<Option<(usize, usize)>> = vec![];
        for p in &state.pending {
            if !order.contains(&p.scope) {
                order.push(p.scope);
            }
        }
        let verify = |items: &[&Pending], scope: Option<(usize, usize)>| -> bool {
            let mut c = Corrector::new(source.clone());
            for p in items {
                parentheses_corrector::apply(&mut c, &p.fix);
            }
            if c.clobbered() {
                return false;
            }
            let corrected = c.rewrite();
            let original = &state.scopes[&scope];
            match scope {
                Some((s, e)) => {
                    let end = (e as isize + corrected.len() as isize - src.text.len() as isize) as usize;
                    let Some(fragment) = corrected.get(s..end) else { return false };
                    parses_equivalently(original, fragment, false, src.version)
                }
                None => parses_equivalently(original, &corrected, true, src.version),
            }
        };
        let mut keep: Vec<&Pending> = vec![];
        for scope in order {
            let group: Vec<&Pending> = state.pending.iter().filter(|p| p.scope == scope).collect();
            let size = match scope {
                Some((s, e)) => src.text[s..e].chars().count(),
                None => src.raw_text.chars().count(),
            };
            if size > MAX_VERIFICATION_FRAGMENT_SIZE || (group.len() > 1 && verify(&group, scope)) {
                keep.extend(group);
            } else {
                keep.extend(group.into_iter().filter(|p| verify(std::slice::from_ref(p), scope)));
            }
        }
        for p in keep {
            cx.add_fix(p.range.0, p.range.1, p.msg.clone(), |c| parentheses_corrector::apply(c, &p.fix));
        }
    }
}
