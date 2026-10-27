use crate::prelude::*;
use ruby_prism::Location;

pub const NAME: &str = "Layout/SpaceAroundKeyword";
pub static COP: SpaceAroundKeyword = SpaceAroundKeyword;
pub struct SpaceAroundKeyword;

/// rubocop-ast OPERATOR_METHODS.
const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~", "+@",
    "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

/// `preceded_by_operator?`: walk parser-shaped ancestors; true at the first and/or/range, or at an
/// operator-method send; false at anything else that is not a send.
fn preceded_by_operator(ancestors: &[Node]) -> bool {
    for i in (0..ancestors.len()).rev() {
        let n = &ancestors[i];
        if frame(n) == Frame::Transparent {
            continue;
        }
        match kind::of(n) {
            kind::AND_NODE | kind::OR_NODE | kind::RANGE_NODE => return true,
            kind::CALL_NODE => {
                // `foo { x }`: parser's block wraps the send, so the send is not x's ancestor.
                if ancestors.get(i + 1).is_some_and(|b| kind::of(b) == kind::BLOCK_NODE) {
                    continue;
                }
                let call = n.as_call_node().unwrap();
                if is_csend(&call) {
                    return false;
                }
                if OPERATOR_METHODS.contains(&method_name(&call)) {
                    return true;
                }
            }
            _ => return false,
        }
    }
    false
}

struct Checker<'a, 'b, 'c, 'pr> {
    cx: &'a mut Cx<'b, 'c>,
    /// Parser ancestors of the node being checked (Prism slice).
    ancestors: &'a [Node<'pr>],
}

impl Checker<'_, '_, '_, '_> {
    fn src(&self) -> &[u8] {
        self.cx.src.text.as_bytes()
    }

    fn space_before_missing(&self, start: usize) -> bool {
        start > 0 && !matches!(self.src()[start - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b | b'(' | b'|' | b'{' | b'[' | b';' | b',' | b'*' | b'=')
    }

    fn space_after_missing(&self, kw: &str, end: usize) -> bool {
        let src = self.src();
        let Some(&c) = src.get(end) else { return false };
        if (matches!(kw, "super" | "yield") && c == b'[')
            || (matches!(kw, "break" | "defined?" | "next" | "not" | "rescue" | "super" | "yield") && c == b'(')
            || src[end..].starts_with(b"&.")
            || (kw == "super" && src[end..].starts_with(b"::"))
        {
            return false;
        }
        !matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b | b';' | b',' | b'#' | b'\\' | b')' | b'}' | b']' | b'.')
    }

    fn keyword(&mut self, loc: Location) {
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let kw = text(&loc);
        if self.space_before_missing(s) && !preceded_by_operator(self.ancestors) {
            self.cx.add_fix(s, e, format!("Space before keyword `{kw}` is missing."), |c| c.insert_before(s, e, " "));
        }
        if self.space_after_missing(kw, e) {
            self.cx.add_fix(s, e, format!("Space after keyword `{kw}` is missing."), |c| c.insert_after(s, e, " "));
        }
    }

    fn opt_keyword(&mut self, loc: Option<Location>) {
        if let Some(loc) = loc {
            self.keyword(loc);
        }
    }

    /// `check_begin` with begin_keyword `do`.
    fn do_keyword(&mut self, loc: Option<Location>) {
        if let Some(loc) = loc.filter(|l| l.as_slice() == b"do") {
            self.keyword(loc);
        }
    }

    fn end_keyword(&mut self, loc: Option<Location>) {
        if let Some(loc) = loc
            && self.space_before_missing(loc.start_offset())
        {
            let (s, e) = (loc.start_offset(), loc.end_offset());
            self.cx.add_fix(s, e, format!("Space before keyword `{}` is missing.", text(&loc)), |c| c.insert_before(s, e, " "));
        }
    }

    /// Block-like: `do` (when the opening is `do`) and the `end` that closes it.
    fn do_end(&mut self, open: Option<Location>, close: Option<Location>) {
        if open.as_ref().is_some_and(|l| l.as_slice() == b"do") {
            self.do_keyword(open);
            self.end_keyword(close);
        }
    }
}

impl Cop for SpaceAroundKeyword {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::AND_NODE,
            kind::OR_NODE,
            kind::BLOCK_NODE,
            kind::LAMBDA_NODE,
            kind::BREAK_NODE,
            kind::NEXT_NODE,
            kind::RETURN_NODE,
            kind::YIELD_NODE,
            kind::SUPER_NODE,
            kind::FORWARDING_SUPER_NODE,
            kind::DEFINED_NODE,
            kind::CASE_NODE,
            kind::CASE_MATCH_NODE,
            kind::ENSURE_NODE,
            kind::FOR_NODE,
            kind::IF_NODE,
            kind::UNLESS_NODE,
            kind::IN_NODE,
            kind::BEGIN_NODE,
            kind::MATCH_REQUIRED_NODE,
            kind::MATCH_PREDICATE_NODE,
            kind::POST_EXECUTION_NODE,
            kind::PRE_EXECUTION_NODE,
            kind::RESCUE_NODE,
            kind::RESCUE_MODIFIER_NODE,
            kind::CALL_NODE,
            kind::UNTIL_NODE,
            kind::WHILE_NODE,
            kind::WHEN_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let all = cx.ancestors;
        // A BlockNode's Prism parent is its call, which parser nests inside the block.
        let ancestors = if kind::of(node) == kind::BLOCK_NODE { &all[..all.len().saturating_sub(1)] } else { all };
        let target = cx.config.target_ruby;
        let mut c = Checker { cx, ancestors };
        match kind::of(node) {
            kind::AND_NODE => {
                let loc = node.as_and_node().unwrap().operator_loc();
                if loc.as_slice() == b"and" {
                    c.keyword(loc);
                }
            }
            kind::OR_NODE => {
                let loc = node.as_or_node().unwrap().operator_loc();
                if loc.as_slice() == b"or" {
                    c.keyword(loc);
                }
            }
            kind::BLOCK_NODE => {
                let b = node.as_block_node().unwrap();
                c.do_end(Some(b.opening_loc()), Some(b.closing_loc()));
            }
            kind::LAMBDA_NODE => {
                let l = node.as_lambda_node().unwrap();
                c.do_end(Some(l.opening_loc()), Some(l.closing_loc()));
            }
            kind::BREAK_NODE => c.keyword(node.as_break_node().unwrap().keyword_loc()),
            kind::NEXT_NODE => c.keyword(node.as_next_node().unwrap().keyword_loc()),
            kind::RETURN_NODE => c.keyword(node.as_return_node().unwrap().keyword_loc()),
            kind::YIELD_NODE => c.keyword(node.as_yield_node().unwrap().keyword_loc()),
            kind::SUPER_NODE => c.keyword(node.as_super_node().unwrap().keyword_loc()),
            kind::FORWARDING_SUPER_NODE => {
                // `super` keyword: the first five bytes of the node.
                let s = node.location().start_offset();
                if c.space_before_missing(s) && !preceded_by_operator(c.ancestors) {
                    c.cx.add_fix(s, s + 5, "Space before keyword `super` is missing.", |k| k.insert_before(s, s + 5, " "));
                }
                // block_brace_with_no_space_style?: `super{` under SpaceBeforeBlockBraces no_space.
                let brace_after = node
                    .as_forwarding_super_node()
                    .unwrap()
                    .block()
                    .is_some_and(|b| b.opening_loc().start_offset() == s + 5);
                let no_space_style = brace_after && {
                    let m = c.cx.config.for_cop("Layout/SpaceBeforeBlockBraces");
                    m.get("Enabled").is_none_or(|v| !matches!(v, Value::Bool(false) | Value::Null))
                        && m.str("EnforcedStyle") == Some("no_space")
                };
                if c.space_after_missing("super", s + 5) && !no_space_style {
                    c.cx.add_fix(s, s + 5, "Space after keyword `super` is missing.", |k| k.insert_after(s, s + 5, " "));
                }
            }
            kind::DEFINED_NODE => c.keyword(node.as_defined_node().unwrap().keyword_loc()),
            kind::CASE_NODE => {
                let n = node.as_case_node().unwrap();
                c.keyword(n.case_keyword_loc());
                c.opt_keyword(n.else_clause().map(|e| e.else_keyword_loc()));
            }
            kind::CASE_MATCH_NODE => {
                let n = node.as_case_match_node().unwrap();
                c.keyword(n.case_keyword_loc());
                c.opt_keyword(n.else_clause().map(|e| e.else_keyword_loc()));
            }
            kind::ENSURE_NODE => c.keyword(node.as_ensure_node().unwrap().ensure_keyword_loc()),
            kind::FOR_NODE => {
                let n = node.as_for_node().unwrap();
                c.do_end(n.do_keyword_loc(), Some(n.end_keyword_loc()));
            }
            kind::IF_NODE => {
                let n = node.as_if_node().unwrap();
                // Ternaries have no `keyword` location in parser.
                let Some(kw) = n.if_keyword_loc() else { return };
                c.keyword(kw);
                if !is_pattern_guard(node, all) {
                    let else_loc = n.subsequent().and_then(|s| match s.as_else_node() {
                        Some(e) => Some(e.else_keyword_loc()),
                        None => s.as_if_node().and_then(|i| i.if_keyword_loc()),
                    });
                    c.opt_keyword(else_loc);
                    c.opt_keyword(n.then_keyword_loc().filter(|l| l.as_slice() == b"then"));
                    c.end_keyword(n.end_keyword_loc());
                }
            }
            kind::UNLESS_NODE => {
                let n = node.as_unless_node().unwrap();
                c.keyword(n.keyword_loc());
                if !is_pattern_guard(node, all) {
                    c.opt_keyword(n.else_clause().map(|e| e.else_keyword_loc()));
                    c.opt_keyword(n.then_keyword_loc().filter(|l| l.as_slice() == b"then"));
                    c.end_keyword(n.end_keyword_loc());
                }
            }
            kind::IN_NODE => c.keyword(node.as_in_node().unwrap().in_loc()),
            kind::BEGIN_NODE => {
                let n = node.as_begin_node().unwrap();
                if let Some(kw) = n.begin_keyword_loc() {
                    c.keyword(kw);
                    c.end_keyword(n.end_keyword_loc());
                }
                // parser `rescue` node: its `else`.
                if n.rescue_clause().is_some() {
                    c.opt_keyword(n.else_clause().map(|e| e.else_keyword_loc()));
                }
            }
            kind::MATCH_REQUIRED_NODE => {
                if target < 3.0 {
                    c.keyword(node.as_match_required_node().unwrap().operator_loc());
                }
            }
            kind::MATCH_PREDICATE_NODE => c.keyword(node.as_match_predicate_node().unwrap().operator_loc()),
            kind::POST_EXECUTION_NODE => c.keyword(node.as_post_execution_node().unwrap().keyword_loc()),
            kind::PRE_EXECUTION_NODE => c.keyword(node.as_pre_execution_node().unwrap().keyword_loc()),
            kind::RESCUE_NODE => c.keyword(node.as_rescue_node().unwrap().keyword_loc()),
            kind::RESCUE_MODIFIER_NODE => c.keyword(node.as_rescue_modifier_node().unwrap().keyword_loc()),
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                if method_name(&call) == "!"
                    && let Some(sel) = call.message_loc().filter(|l| l.as_slice() == b"not")
                {
                    c.keyword(sel);
                }
            }
            // `begin ... end while x` is parser `while_post`, which the cop does not handle.
            kind::UNTIL_NODE => {
                let n = node.as_until_node().unwrap();
                if n.is_begin_modifier() {
                    return;
                }
                c.do_end(n.do_keyword_loc(), n.closing_loc());
                c.keyword(n.keyword_loc());
            }
            kind::WHILE_NODE => {
                let n = node.as_while_node().unwrap();
                if n.is_begin_modifier() {
                    return;
                }
                c.do_end(n.do_keyword_loc(), n.closing_loc());
                c.keyword(n.keyword_loc());
            }
            kind::WHEN_NODE => c.keyword(node.as_when_node().unwrap().keyword_loc()),
            _ => {}
        }
    }
}
