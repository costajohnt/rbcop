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
