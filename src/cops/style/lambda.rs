use crate::prelude::*;
use ruby_prism::{BlockParametersNode, Location};

pub const NAME: &str = "Style/Lambda";
pub static COP: Lambda = Lambda;
pub struct Lambda;

fn span(l: &Location) -> (usize, usize) {
    (l.start_offset(), l.end_offset())
}

/// Parser `args` children as source: regular arguments, then `; shadow` ones.
fn lambda_arg_string(bp: &BlockParametersNode) -> String {
    let mut regular: Vec<&str> = Vec::new();
    if let Some(p) = bp.parameters() {
        regular = parameter_nodes(&p).iter().map(node_text).collect();
    }
    let shadow: Vec<&str> = bp.locals().iter().map(|l| node_text(&l)).collect();
    let mut s = regular.join(", ");
    if !shadow.is_empty() {
        s.push_str(&format!("; {}", shadow.join(", ")));
    }
    s
}

/// Parser `args` is empty (no parameters and no shadow args).
fn args_empty(bp: &BlockParametersNode) -> bool {
    bp.parameters().is_none_or(|p| parameter_nodes(&p).is_empty()) && bp.locals().iter().next().is_none()
}

impl Cop for Lambda {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("line_count_dependent");
        let src = cx.src;
        if let Some(call) = node.as_call_node() {
            // `lambda { }`: the send source must be exactly `lambda`
            let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
            if method_name(&call) != "lambda" {
                return;
            }
            let (s, e) = send_range(&call);
            if &src.text[s..e] != "lambda" {
                return;
            }
            let multiline = src.line_of(block.opening_loc().start_offset()) != src.line_of(block.closing_loc().start_offset());
            let offending = match style {
                "literal" => true,
                "line_count_dependent" => !multiline,
                _ => false,
            };
            if !offending {
                return;
            }
            let msg = format!("Use the `-> {{ ... }}` lambda literal syntax for {} lambdas.", modifier(style, multiline));
            // autocorrect_method_to_literal
            let params = block.parameters().and_then(|p| p.as_block_parameters_node());
            let args = params.as_ref().filter(|bp| !args_empty(bp)).map(|bp| {
                let closing = bp.closing_loc().map_or(bp.location().end_offset(), |c| c.end_offset());
                (format!("({})", lambda_arg_string(bp)), block.opening_loc().end_offset(), closing)
            });
            cx.add_fix(s, e, msg, |c| {
                c.replace(s, e, "->");
                if let Some((arg_str, rs, re)) = &args {
                    c.insert_after(s, e, arg_str);
                    c.remove(*rs, *re);
                }
            });
            return;
        }
        let l = node.as_lambda_node().unwrap();
        let (open, close) = (span(&l.opening_loc()), span(&l.closing_loc()));
        let multiline = src.line_of(open.0) != src.line_of(close.0);
        let offending = match style {
            "lambda" => true,
            "line_count_dependent" => multiline,
            _ => false,
        };
        if !offending {
            return;
        }
        let msg = format!("Use the `lambda` method for {} lambdas.", modifier(style, multiline));
        let sel = span(&l.operator_loc());
        let params = l.parameters();
        // parser block (not numblock/itblock) and its `args` node
        let bp = params.as_ref().and_then(|p| p.as_block_parameters_node());
        let block_type = params.is_none() || bp.is_some();
        let empty = bp.as_ref().is_none_or(args_empty);
        let args_loc = bp.as_ref().map(|b| span(&b.location()));
        let args_open = bp.as_ref().and_then(|b| b.opening_loc()).map(|o| o.start_offset());
        let args_close = bp.as_ref().and_then(|b| b.closing_loc()).map(|o| o.end_offset());
        let parenthesized = bp.as_ref().and_then(|b| b.opening_loc()).is_some_and(|o| o.as_slice() == b"(");
        let arg_str = bp.as_ref().map(lambda_arg_string).unwrap_or_default();
        let braces = l.opening_loc().as_slice() == b"{";
        let unparenthesized_arg = arg_to_unparenthesized_call(node, cx.ancestors);
        let bytes = src.text.as_bytes();
        let separating_space = bytes.get(open.0 + 2).is_some_and(|c| c.is_ascii_whitespace());
        cx.add_fix(sel.0, sel.1, msg, |c| {
            // remove_unparenthesized_whitespace
            if let (false, false, Some((as_, ae))) = (empty, parenthesized, args_loc) {
                c.remove_preceding(as_, ae, as_ - sel.1);
                if open.0 > ae + 1 {
                    c.remove_preceding(open.0, open.1, open.0 - ae - 1);
                }
            }
            if block_type {
                let needs_space = (Some(open.0) == args_close && Some(sel.1) == args_open) || open.0 == sel.1;
                if needs_space {
                    c.insert_before(open.0, open.1, " ");
                }
                if let Some((as_, ae)) = args_loc {
                    c.remove(as_, ae);
                }
            }
            c.replace(sel.0, sel.1, "lambda");
            if !braces && unparenthesized_arg {
                if !separating_space {
                    c.insert_after(open.0, open.1, " ");
                }
                c.replace(open.0, open.1, "{");
                c.replace(close.0, close.1, "}");
            }
            if !empty {
                c.insert_after(open.0, open.1, &format!(" |{arg_str}|"));
            }
        });
    }
}

fn modifier(style: &str, multiline: bool) -> &'static str {
    match (style, multiline) {
        ("line_count_dependent", true) => "multiline",
        ("line_count_dependent", false) => "single line",
        _ => "all",
    }
}

/// LambdaLiteralToMethodCorrector#arg_to_unparenthesized_call?: an argument (possibly a hash
/// pair value) of a `send` without parentheses.
fn arg_to_unparenthesized_call(node: &Node, ancestors: &[Node]) -> bool {
    let mut i = ancestors.len();
    let mut current = (node.location().start_offset(), node.location().end_offset());
    let next = |i: &mut usize| -> Option<usize> {
        while *i > 0 {
            *i -= 1;
            let k = kind::of(&ancestors[*i]);
            let single_stmt = k == kind::STATEMENTS_NODE && frame(&ancestors[*i]) == Frame::Transparent;
            if k != kind::ARGUMENTS_NODE && !single_stmt {
                return Some(*i);
            }
            if single_stmt {
                // a lone statement under parens/program etc. is that parent's child directly
                continue;
            }
        }
        None
    };
    let Some(mut p) = next(&mut i) else { return false };
    if kind::of(&ancestors[p]) == kind::ASSOC_NODE {
        let Some(h) = next(&mut i) else { return false };
        current = (ancestors[h].location().start_offset(), ancestors[h].location().end_offset());
        let Some(pp) = next(&mut i) else { return false };
        p = pp;
    }
    let Some(call) = ancestors[p].as_call_node() else { return false };
    if is_csend(&call) || call.opening_loc().is_some_and(|o| o.as_slice() == b"(") {
        return false;
    }
    // sibling_index > 1: not the receiver
    !call.receiver().is_some_and(|r| (r.location().start_offset(), r.location().end_offset()) == current)
}
