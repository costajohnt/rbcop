use crate::prelude::*;
use helpers::endless_method_body_help as endless_help;
use helpers::line_break_corrector::{break_line_before, move_comment};

pub const NAME: &str = "Style/SingleLineMethods";
pub static COP: SingleLineMethods = SingleLineMethods;
pub struct SingleLineMethods;

const COMPARISON: &[&str] = &["==", "===", "!=", "<=", ">=", ">", "<"];
const ARITHMETIC: &[&str] = &["+", "-", "*", "/", "%", "**"];

/// Parser body of a def: the single statement, a `begin` (StatementsNode of 2+), or rescue/ensure.
fn parser_body<'pr>(body: Node<'pr>) -> Node<'pr> {
    if let Some(s) = body.as_statements_node() {
        let mut it = s.body().iter();
        if let (Some(first), None) = (it.next(), it.next()) {
            return first;
        }
    }
    body
}

/// SingleLineMethods#require_parentheses?: a send with arguments, not arithmetic or a comparison.
fn require_parentheses(body: &Node) -> bool {
    let Some(call) = body.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
    else {
        return false;
    };
    let name = method_name(&call);
    !ARITHMETIC.contains(&name) && !send_args(&call).is_empty() && !COMPARISON.contains(&name)
}

impl Cop for SingleLineMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let n = node.as_def_node().unwrap();
        let src = cx.src;
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        if src.line_of(start) != src.line_of(end) || n.equal_loc().is_some() {
            return;
        }
        let body = n.body().map(parser_body);
        if body.is_none() && cx.cfg.bool("AllowIfMethodIsEmpty") == Some(true) {
            return;
        }
        let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
        let arg_call = endless_help::argument_receiving_call(cx.ancestors);
        let endless = cx.config.target_ruby >= 3.0
            && helpers::alignment::cop_enabled(cx, "Style/EndlessMethod")
            && cx.config.for_cop("Style/EndlessMethod").str("EnforcedStyle") != Some("disallow")
            && body.as_ref().is_some_and(|b| {
                // unsupported_endless_method_body?: a basic conditional, return, break or next
                !matches!(
                    kind::of(b),
                    kind::IF_NODE
                        | kind::UNLESS_NODE
                        | kind::WHILE_NODE
                        | kind::UNTIL_NODE
                        | kind::RETURN_NODE
                        | kind::BREAK_NODE
                        | kind::NEXT_NODE
                ) && !(name.ends_with('=') && !COMPARISON.contains(&name))
                    && !captures_following_code(cx.src, cx.ancestors, (start, end))
                    // The body gets parenthesized when it becomes endless, so it is no longer a command.
                    && endless_help::endless_method_body(b, arg_call.as_ref(), cx.config.target_ruby, require_parentheses(b))
            });
        let width = helpers::alignment::configured_indentation_width(cx);
        let keyword_col = src.column(n.def_keyword_loc().start_offset());
        let end_kw = n.end_keyword_loc().map(|l| (l.start_offset(), l.end_offset()));
        let replacement = if endless { Some(endless_source(src, &n,body.as_ref().unwrap(), name)) } else { None };
        let parts: Vec<(usize, usize)> = match &body {
            None => vec![],
            Some(b) if b.as_parentheses_node().is_some() => vec![body_range(b)],
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().map(|c| (c.location().start_offset(), c.location().end_offset())).collect(),
                None => vec![body_range(b)],
            },
        };
        cx.add_fix(start, end, "Avoid single-line method definitions.", |c| {
            if let Some(r) = &replacement {
                c.replace(start, end, r);
                return;
            }
            for &p in &parts {
                break_line_before(c, p, keyword_col, width, 1);
            }
            if let Some(e) = end_kw {
                break_line_before(c, e, keyword_col, width, 0);
            }
            move_comment(c, src, (start, end), keyword_col);
        });
    }
}

/// `captures_following_code?` for the def (range `def_range`, Prism `ancestors`): an endless def
/// would take in the code after it, as in `def foo; bar; end && baz` or `a && def foo; bar; end || baz`.
fn captures_following_code(src: &crate::source::Source, ancestors: &[Node], def_range: (usize, usize)) -> bool {
    use helpers::parser_parent::{is_logical_operator, is_ternary, parser_parent_index};
    let (mut i, mut node) = (ancestors.len(), def_range);
    loop {
        let Some(pi) = parser_parent_index(ancestors, i) else { return false };
        let parent = &ancestors[pi];
        // A BlockNode stands for the parser `block` (its call's whole range); a call reached from
        // its receiver or arguments is the parser `send`.
        let (p_range, send_with_block) = match parent.as_call_node() {
            Some(c) => (send_range(&c), c.block().is_some_and(|b| b.as_block_node().is_some())),
            None => {
                let whole = if kind::of(parent) == kind::BLOCK_NODE { &ancestors[pi - 1] } else { parent };
                ((whole.location().start_offset(), whole.location().end_offset()), false)
            }
        };
        let p_end = p_range.1;
        if p_end == node.1 {
            if send_with_block {
                // The parser parent is the block, which ends later and never extends the body.
                return false;
            }
            (i, node) = (pi, p_range);
            continue;
        }
        let is_node = |n: Option<Node>| n.is_some_and(|n| n.location().start_offset() == node.0 && n.location().end_offset() == node.1);
        // `parent.children.first.equal?(node) && extends_endless_body?(parent, node)`
        return match parent {
            Node::AndNode { .. } => is_logical_operator(parent) && is_node(Some(parent.as_and_node().unwrap().left())),
            Node::OrNode { .. } => is_logical_operator(parent) && is_node(Some(parent.as_or_node().unwrap().left())),
            Node::IfNode { .. } => is_ternary(parent) && is_node(Some(parent.as_if_node().unwrap().predicate())),
            Node::RescueModifierNode { .. } => is_node(Some(parent.as_rescue_modifier_node().unwrap().expression())),
            Node::BeginNode { .. } => {
                // The parser `rescue` inside a kwbegin: the modifier-like one-line form.
                let b = parent.as_begin_node().unwrap();
                let only = b.statements().and_then(|s| {
                    let mut it = s.body().iter();
                    match (it.next(), it.next()) {
                        (Some(n), None) => Some(n),
                        _ => None,
                    }
                });
                b.rescue_clause().is_some_and(|r| src.line_of(r.keyword_loc().start_offset()) == src.line_of(node.0))
                    && is_node(only)
            }
            Node::CallNode { .. } => is_node(parent.as_call_node().unwrap().receiver()),
            Node::RangeNode { .. } => is_node(parent.as_range_node().unwrap().left()),
            _ => false,
        };
    }
}

/// SingleLineMethods#correct_to_endless replacement.
fn endless_source(src: &crate::source::Source, n: &ruby_prism::DefNode, body: &Node, name: &str) -> String {
    let receiver = n.receiver().map(|r| format!("{}.", node_text(&r))).unwrap_or_default();
    // parser `args` source: includes the parentheses when there are any.
    let arguments = match (n.parameters(), n.lparen_loc(), n.rparen_loc()) {
        (Some(_), Some(l), Some(r)) => src.slice(l.start_offset(), r.end_offset()).to_string(),
        // endless_arguments: unparenthesized parameters get parentheses.
        (Some(p), _, _) => format!("({})", node_text(&p.as_node())),
        _ => "()".to_string(),
    };
    format!("def {receiver}{name}{arguments} = {}", body_source(src, body))
}

/// SingleLineMethods#method_body_source.
fn body_source(src: &crate::source::Source, body: &Node) -> String {
    if let Some(call) = body.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none())) {
        let name = method_name(&call);
        let mut args: Vec<&str> = call_args(&call).iter().map(|a| node_text(a)).collect();
        if let Some(b) = call.block() {
            args.push(node_text(&b));
        }
        if !ARITHMETIC.contains(&name) && !args.is_empty() && !COMPARISON.contains(&name) {
            let s = format!("{name}({})", args.join(", "));
            return match call.receiver() {
                Some(r) => format!("{}.{s}", node_text(&r)),
                None => s,
            };
        }
    }
    let (s, e) = body_range(body);
    src.slice(s, e).to_string()
}
