use crate::helpers::node_equality::same;
use crate::helpers::node_types::is_operator_method;
use crate::helpers::parser_parent::parser_parent;
use crate::prelude::*;
use ruby_prism::{CallNode, StatementsNode};

pub const NAME: &str = "Style/RedundantCondition";
pub static COP: RedundantCondition = RedundantCondition;
pub struct RedundantCondition;

const MSG: &str = "Use double pipes `||` instead.";
const REDUNDANT_CONDITION: &str = "This condition is not needed.";

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// A parser branch: the single statement, a `begin` (the StatementsNode) for several, or nil.
fn branch<'pr>(stmts: Option<StatementsNode<'pr>>) -> Option<Node<'pr>> {
    let s = stmts?;
    let mut it = s.body().iter();
    let first = it.next()?;
    if it.next().is_some() { Some(s.as_node()) } else { Some(first) }
}

/// A parser `send` (not csend, not a call with a literal block).
fn send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

fn eq(src: &crate::source::Source, a: &Node, b: &Node) -> bool {
    same(src, range(a), range(b))
}

fn opt_eq(src: &crate::source::Source, a: Option<&Node>, b: Option<&Node>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => eq(src, a, b),
        _ => false,
    }
}

fn parenthesized(call: &CallNode) -> bool {
    call.opening_loc().is_some_and(|l| l.as_slice() == b"(")
}

/// rubocop-ast nodes with no specialised class (`instance_of?(AST::Node)`).
fn generic_node(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::STATEMENTS_NODE
        | kind::PARENTHESES_NODE
        | kind::TRUE_NODE
        | kind::FALSE_NODE
        | kind::NIL_NODE
        | kind::SELF_NODE
        | kind::INTERPOLATED_SYMBOL_NODE
        | kind::NUMBERED_REFERENCE_READ_NODE
        | kind::BACK_REFERENCE_READ_NODE
        | kind::MATCH_WRITE_NODE
        | kind::MATCH_PREDICATE_NODE
        | kind::MATCH_REQUIRED_NODE
        | kind::RETRY_NODE
        | kind::REDO_NODE
        | kind::SPLAT_NODE
        | kind::BLOCK_ARGUMENT_NODE
        | kind::SOURCE_ENCODING_NODE
        | kind::FLIP_FLOP_NODE
        | kind::MATCH_LAST_LINE_NODE
        | kind::INTERPOLATED_MATCH_LAST_LINE_NODE
    )
}

/// `asgn_type?` (lvasgn ivasgn cvasgn gvasgn casgn): (name, value).
fn asgn<'pr>(node: &Node<'pr>) -> Option<(Vec<u8>, Node<'pr>)> {
    if let Some(n) = node.as_local_variable_write_node() {
        return Some((n.name().as_slice().to_vec(), n.value()));
    }
    if let Some(n) = node.as_instance_variable_write_node() {
        return Some((n.name().as_slice().to_vec(), n.value()));
    }
    if let Some(n) = node.as_class_variable_write_node() {
        return Some((n.name().as_slice().to_vec(), n.value()));
    }
    if let Some(n) = node.as_global_variable_write_node() {
        return Some((n.name().as_slice().to_vec(), n.value()));
    }
    if let Some(n) = node.as_constant_write_node() {
        return Some((n.name().as_slice().to_vec(), n.value()));
    }
    if let Some(n) = node.as_constant_path_write_node() {
        let name = n.target().name().map(|c| c.as_slice().to_vec()).unwrap_or_default();
        return Some((name, n.value()));
    }
    None
}

/// `argument_with_operator?`
fn argument_with_operator(arg: &Node) -> bool {
    match kind::of(arg) {
        kind::SPLAT_NODE | kind::BLOCK_ARGUMENT_NODE | kind::FORWARDING_ARGUMENTS_NODE => true,
        kind::HASH_NODE => arg.as_hash_node().unwrap().elements().iter().next().is_some_and(|e| e.as_assoc_splat_node().is_some()),
        kind::KEYWORD_HASH_NODE => {
            arg.as_keyword_hash_node().unwrap().elements().iter().next().is_some_and(|e| e.as_assoc_splat_node().is_some())
        }
        _ => false,
    }
}

fn single_argument_method(node: &Node) -> bool {
    let Some(call) = send(node) else { return false };
    if method_name(&call) == "[]" {
        return false;
    }
    let args = send_args(&call);
    args.len() == 1 && !argument_with_operator(&args[0])
}

struct Parts<'pr> {
    cond: Node<'pr>,
    /// Raw parser children (unless swaps them).
    raw_if: Option<Node<'pr>>,
    raw_else: Option<Node<'pr>>,
    /// rubocop-ast `if_branch` / `else_branch`.
    if_branch: Option<Node<'pr>>,
    else_branch: Option<Node<'pr>>,
    ternary: bool,
    keyword_if: bool,
}

impl Parts<'_> {
    fn branches_have_method(&self, src: &crate::source::Source) -> bool {
        let (Some(i), Some(e)) = (&self.if_branch, &self.else_branch) else { return false };
        if !single_argument_method(i) || !single_argument_method(e) {
            return false;
        }
        let (i, e) = (send(i).unwrap(), send(e).unwrap());
        method_name(&i) == method_name(&e) && opt_eq(src, i.receiver().as_ref(), e.receiver().as_ref())
    }

    fn branches_have_assignment(&self) -> bool {
        let (Some(i), Some(e)) = (&self.raw_if, &self.raw_else) else { return false };
        matches!((asgn(i), asgn(e)), (Some((a, _)), Some((b, _))) if a == b)
    }
}

/// `require_parentheses?`
fn require_parentheses(node: &Node) -> bool {
    let modifier = match kind::of(node) {
        kind::IF_NODE => {
            let n = node.as_if_node().unwrap();
            n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && n.end_keyword_loc().is_none()
        }
        kind::UNLESS_NODE => node.as_unless_node().unwrap().end_keyword_loc().is_none(),
        kind::WHILE_NODE => node.as_while_node().unwrap().closing_loc().is_none(),
        kind::UNTIL_NODE => node.as_until_node().unwrap().closing_loc().is_none(),
        _ => false,
    };
    let semantic = match kind::of(node) {
        kind::AND_NODE => node.as_and_node().unwrap().operator_loc().as_slice() == b"and",
        kind::OR_NODE => node.as_or_node().unwrap().operator_loc().as_slice() == b"or",
        _ => false,
    };
    modifier || semantic || matches!(kind::of(node), kind::RANGE_NODE | kind::RESCUE_MODIFIER_NODE) || low_precedence_keyword(node)
}

/// `low_precedence_keyword?`: e.g. `not foo`, `return foo`, and `yield foo`.
fn low_precedence_keyword(node: &Node) -> bool {
    // A `return`'s arguments: those of a single parenthesized argument when it is one.
    let wrapped = |args: Option<ruby_prism::ArgumentsNode>| {
        let Some(args) = args else { return false };
        let list: Vec<Node> = args.arguments().iter().collect();
        match list.as_slice() {
            [only] => only.as_parentheses_node().is_none_or(|p| p.body().is_some()),
            l => !l.is_empty(),
        }
    };
    match node {
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            !is_csend(&c) && method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"not")
        }
        Node::ReturnNode { .. } => wrapped(node.as_return_node().unwrap().arguments()),
        Node::NextNode { .. } => wrapped(node.as_next_node().unwrap().arguments()),
        Node::BreakNode { .. } => wrapped(node.as_break_node().unwrap().arguments()),
        Node::YieldNode { .. } => {
            let y = node.as_yield_node().unwrap();
            y.arguments().is_some() && y.lparen_loc().is_none()
        }
        Node::SuperNode { .. } => {
            let s = node.as_super_node().unwrap();
            let has_args = s.arguments().is_some() || s.block().is_some_and(|b| b.as_block_argument_node().is_some());
            has_args && s.lparen_loc().is_none()
        }
        _ => false,
    }
}

/// `require_braces?`: an unbraced hash.
fn require_braces(node: &Node) -> bool {
    node.as_keyword_hash_node().is_some()
}

/// `binary_operator?`: any binary operator binds tighter than `||`, so the fallback must be
/// parenthesized as its argument, e.g. `stack << (foo || :new)`.
fn binary_operator(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        c.block().is_none_or(|b| b.as_block_node().is_none())
            && helpers::node_types::is_binary_operation(&c)
            && c.call_operator_loc().is_none_or(|d| d.as_slice() == b"::")
    })
}

fn wrap_or_brace(node: &Node) -> String {
    if require_parentheses(node) {
        format!("({})", node_text(node))
    } else if require_braces(node) {
        format!("{{ {} }}", node_text(node))
    } else {
        node_text(node).to_string()
    }
}

impl Cop for RedundantCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(p) = self.offense(cx, node, cx.ancestors) else { return };
        // An enclosing `if` that is itself an offense is corrected instead.
        let nested = (0..cx.ancestors.len()).any(|i| {
            matches!(kind::of(&cx.ancestors[i]), kind::IF_NODE | kind::UNLESS_NODE)
                && self.offense(cx, &cx.ancestors[i], &cx.ancestors[..i]).is_some()
        });
        if nested {
            return;
        }
        let src = cx.src;

        let message = if p.else_branch.is_none() { REDUNDANT_CONDITION } else { MSG };
        let (node_start, node_end) = range(node);
        let has_method = p.branches_have_method(src);
        let (start, end) = if !p.ternary || has_method {
            (node_start, node_end)
        } else {
            let n = node.as_if_node().unwrap();
            let colon = n.subsequent().and_then(|s| s.as_else_node()).unwrap().else_keyword_loc();
            (n.then_keyword_loc().unwrap().start_offset(), colon.end_offset())
        };

        // autocorrect
        let (first_line, last_line) = (src.line_of(node_start), src.line_of(node_end));
        let has_comments = src.comments.iter().any(|c| {
            let l = src.line_of(c.start);
            l >= first_line && l < last_line && c.start >= node_start
        });
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        if !has_comments {
            if p.ternary && !has_method {
                edits.push((start, end, "||".to_string()));
                if let Some(e) = p.else_branch.as_ref().filter(|e| e.as_range_node().is_some()) {
                    let (s, e) = range(e);
                    edits.push((s, s, "(".to_string()));
                    edits.push((e, e, ")".to_string()));
                }
            } else if p.else_branch.is_none() {
                let text = p.if_branch.as_ref().map_or("", |b| node_text(b));
                edits.push((node_start, node_end, text.to_string()));
            } else if let Some(t) = self.make_ternary_form(cx, &p) {
                edits.push((node_start, node_end, t));
            }
        }
        cx.add_fix(start, end, message, |c| {
            for (s, e, t) in &edits {
                if s == e {
                    if t == "(" {
                        c.insert_before(*s, *s, t);
                    } else {
                        c.insert_after(*s, *s, t);
                    }
                } else {
                    c.replace(*s, *e, t);
                }
            }
        });
    }
}

impl RedundantCondition {
    /// `redundant_condition_offense?`: the node's parts when it is an offense.
    fn offense<'pr>(&self, cx: &Cx, node: &Node<'pr>, ancestors: &[Node]) -> Option<Parts<'pr>> {
        if is_pattern_guard(node, ancestors) {
            return None;
        }
        let p = if let Some(n) = node.as_if_node() {
            let kw = n.if_keyword_loc();
            if kw.as_ref().is_some_and(|k| k.as_slice() == b"elsif") {
                return None;
            }
            if kw.is_some() && n.end_keyword_loc().is_none() {
                return None; // modifier
            }
            if n.subsequent().is_some_and(|s| s.as_if_node().is_some()) {
                return None; // elsif_conditional?
            }
            let else_b = || branch(n.subsequent().and_then(|s| s.as_else_node()).and_then(|e| e.statements()));
            let then_b = || branch(n.statements());
            Parts {
                cond: n.predicate(),
                raw_if: then_b(),
                raw_else: else_b(),
                if_branch: then_b(),
                else_branch: else_b(),
                ternary: kw.is_none(),
                keyword_if: kw.is_some(),
            }
        } else {
            let n = node.as_unless_node()?;
            n.end_keyword_loc()?;
            let then_b = || branch(n.statements());
            let else_b = || branch(n.else_clause().and_then(|e| e.statements()));
            Parts {
                cond: n.predicate(),
                raw_if: else_b(),
                raw_else: then_b(),
                if_branch: then_b(),
                else_branch: else_b(),
                ternary: false,
                keyword_if: false,
            }
        };
        let src = cx.src;
        if let Some(e) = &p.raw_else {
            if matches!(kind::of(e), kind::IF_NODE | kind::UNLESS_NODE) {
                return None;
            }
            if send(e).is_some_and(|c| method_name(&c) == "[]=") {
                return None;
            }
        }
        let synonymous = p.raw_if.as_ref().is_some_and(|i| eq(src, &p.cond, i))
            || self.true_type_and_else_is_not(cx, &p)
            || (p.branches_have_assignment()
                && asgn(p.raw_if.as_ref().unwrap()).is_some_and(|(_, v)| eq(src, &p.cond, &v)))
            || (p.branches_have_method(src)
                && send(p.if_branch.as_ref().unwrap()).is_some_and(|c| {
                    method_name(&c) != "[]" && eq(src, &p.cond, &send_args(&c)[0])
                }));
        if !synonymous {
            return None;
        }
        let else_ok = p.ternary
            || p.raw_else.as_ref().is_none_or(|e| {
                !generic_node(e) || src.line_of(range(e).0) == src.line_of(range(e).1)
            });
        else_ok.then_some(p)
    }

    /// `if_branch_is_true_type_and_else_is_not?`
    fn true_type_and_else_is_not(&self, cx: &Cx, p: &Parts) -> bool {
        if !(p.ternary || p.keyword_if) {
            return false;
        }
        let Some(cond) = p.cond.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
        else {
            return false;
        };
        let name = method_name(&cond);
        if !name.ends_with('?') || helpers::allowed::allowed_method(cx.cfg, name) {
            return false;
        }
        p.if_branch.as_ref().is_some_and(|i| i.as_true_node().is_some())
            && p.else_branch.as_ref().is_some_and(|e| e.as_true_node().is_none())
    }

    fn make_ternary_form(&self, cx: &Cx, p: &Parts) -> Option<String> {
        let src = cx.src;
        let raw_if = p.raw_if.as_ref()?;
        let raw_else = p.raw_else.as_ref()?;
        let has_method = p.branches_have_method(src);
        let arith = has_method && binary_operator(raw_if);
        let if_paren = raw_if.as_call_node().is_some_and(|c| parenthesized(&c));
        let if_source = if has_method && if_paren {
            let s = node_text(raw_if);
            s.strip_suffix(')').unwrap_or(s).to_string()
        } else if arith {
            let c = raw_if.as_call_node().unwrap();
            let arg = call_args(&c).into_iter().next()?;
            format!("{} {} ({}", node_text(&c.receiver()?), method_name(&c), node_text(&arg))
        } else if raw_if.as_true_node().is_some() {
            // condition.arguments raises on a non-call condition: RuboCop loses the offense.
            let c = p.cond.as_call_node()?;
            let args = send_args(&c);
            if args.is_empty() || parenthesized(&c) {
                node_text(&p.cond).to_string()
            } else {
                let (cs, ce) = range(&p.cond);
                let sel_end = c.message_loc().map_or(cs, |m| m.end_offset());
                let arg_start = range(&args[0]).0;
                format!("{}({})", &src.text[cs..sel_end], &src.text[arg_start..ce])
            }
        } else {
            node_text(raw_if).to_string()
        };
        let else_source = if arith {
            let c = raw_else.as_call_node()?;
            format!("{})", node_text(&send_args(&c).into_iter().next()?))
        } else if has_method {
            let c = send(raw_else)?;
            wrap_or_brace(&send_args(&c)[0])
        } else if require_parentheses(raw_else) {
            format!("({})", node_text(raw_else))
        } else if let Some(c) = send(raw_else).filter(|c| {
            let name = method_name(c);
            !send_args(c).is_empty()
                && !parenthesized(c)
                && !is_operator_method(name)
                && !(name.ends_with('=') && !matches!(name, "==" | "===" | "!=" | "<=" | ">="))
        }) {
            let args: Vec<&str> = send_args(&c).iter().map(|a| node_text(a)).collect();
            format!("{}({})", method_name(&c), args.join(", "))
        } else if p.branches_have_assignment() {
            let (_, value) = asgn(raw_else)?;
            wrap_or_brace(&value)
        } else {
            node_text(raw_else).to_string()
        };
        let mut form = format!("{if_source} || {else_source}");
        if has_method && if_paren {
            form.push(')');
        }
        if parser_parent(cx.ancestors).and_then(|n| n.as_call_node()).is_some_and(|c| !is_csend(&c)) {
            form = format!("({form})");
        }
        Some(form)
    }
}

