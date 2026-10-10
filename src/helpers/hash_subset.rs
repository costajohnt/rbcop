//! RuboCop's HashSubset mixin (Style/HashExcept, Style/HashSlice).

use crate::prelude::*;
use ruby_prism::CallNode;

/// `on_send` / `on_csend` for `reject` / `select` / `filter` with a block; `except` picks
/// HashExcept (`semantically_except_method?`) over HashSlice.
pub fn check(cx: &mut Cx, node: &Node, except: bool) {
    let call = node.as_call_node().unwrap();
    let method = call.name().as_slice();
    if !matches!(method, b"reject" | b"select" | b"filter") || call.arguments().is_some() {
        return;
    }
    let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
    // (args (arg _key) (arg _))
    let Some(params) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
    if params.locals().iter().next().is_some() {
        return;
    }
    let Some(params) = params.parameters() else { return };
    let req: Vec<Node> = params.requireds().iter().collect();
    if req.len() != 2
        || params.optionals().iter().next().is_some()
        || params.rest().is_some_and(|r| r.as_implicit_rest_node().is_none())
        || params.posts().iter().next().is_some()
        || params.keywords().iter().next().is_some()
        || params.keyword_rest().is_some()
        || params.block().is_some()
    {
        return;
    }
    let (Some(key), Some(value)) = (req[0].as_required_parameter_node(), req[1].as_required_parameter_node()) else {
        return;
    };
    let (key_name, value_name) = (key.name().as_slice(), value.name().as_slice());
    let Some(body) = block.body().and_then(|b| b.as_statements_node()) else { return };
    let mut stmts = body.body().iter();
    let (Some(body), None) = (stmts.next(), stmts.next()) else { return };
    let Some(body) = body.as_call_node() else { return };
    let is_lvar = |n: &Node, name: &[u8]| n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name);
    // (send {(lvar _key) $_ _ | _ $_ (lvar _key)}), possibly under `(send ... :!)`.
    let matches = |s: &CallNode| {
        if is_csend(s) || s.block().is_some() {
            return false;
        }
        let args = call_args(s);
        if args.len() != 1 {
            return false;
        }
        s.receiver().is_some_and(|r| is_lvar(&r, key_name)) || is_lvar(&args[0], key_name)
    };
    let negated = body.name().as_slice() == b"!";
    let send = if matches(&body) {
        body
    } else if negated
        && !is_csend(&body)
        && body.arguments().is_none()
        && body.block().is_none()
        && let Some(inner) = body.receiver().and_then(|r| r.as_call_node())
        && matches(&inner)
    {
        inner
    } else {
        return;
    };
    let subset = send.name().as_slice();
    let active_support = cx.config.all_cops().truthy("ActiveSupportExtensionsEnabled");
    let supported = matches!(subset, b"==" | b"!=" | b"eql?" | b"include?")
        || (active_support && matches!(subset, b"in?" | b"exclude?"));
    if !supported {
        return;
    }
    let arg = call_args(&send).into_iter().next().unwrap();
    // range_include?: a nil receiver raises there.
    let Some(receiver) = send.receiver() else { return };
    if arg.as_range_node().is_some() {
        return;
    }
    let mut r = send.receiver().unwrap();
    while let Some(p) = r.as_parentheses_node() {
        // `receiver.child_nodes.first` of a parser `begin`.
        let Some(b) = p.body() else { break };
        r = match b.as_statements_node() {
            Some(s) => match s.body().iter().next() {
                Some(f) => f,
                None => break,
            },
            None => b,
        };
    }
    if r.as_range_node().is_some() {
        return;
    }
    let key_src = node_text(&req[0]);
    let slices = |n: &Node| {
        // using_value_variable?
        !(is_lvar(&receiver, value_name) || is_lvar(&arg, value_name)) && node_text(n) == key_src
    };
    let ok = match subset {
        b"include?" | b"exclude?" => slices(&arg),
        b"in?" => slices(&receiver),
        _ => true,
    };
    if !ok {
        return;
    }
    // except_key: the operand that is not the key.
    let except_key = if node_text(&receiver) == key_src { &arg } else { &receiver };
    // safe_to_register_offense? looks at the unnegated body only.
    if !negated
        && matches!(subset, b"==" | b"!=")
        && !(except_key.as_symbol_node().is_some() || (except_key.as_string_node().is_some() && !is_parser_dstr(except_key)))
    {
        return;
    }
    // semantically_except_method?
    let included = |neg: bool| if neg { subset == b"exclude?" } else { matches!(subset, b"include?" | b"in?") };
    let is_except = if method == b"reject" {
        matches!(subset, b"==" | b"eql?") || included(negated)
    } else {
        subset == b"!=" || included(!negated)
    };
    if is_except != except {
        return;
    }
    let preferred = format!("{}({})", if except { "except" } else { "slice" }, key_source(except_key));
    let start = call.message_loc().unwrap().start_offset();
    let end = block.closing_loc().end_offset();
    cx.add_fix(start, end, format!("Use `{preferred}` instead."), |c| c.replace(start, end, &preferred));
}

/// `except_key_source`.
fn key_source(key: &Node) -> String {
    if let Some(a) = key.as_array_node() {
        let percent = a.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%"));
        let parts: Vec<String> = a
            .elements()
            .iter()
            .map(|v| if percent { decorate_source(&v) } else { node_text(&v).to_string() })
            .collect();
        return parts.join(", ");
    }
    if helpers::literal_node::literal(key) {
        node_text(key).to_string()
    } else {
        format!("*{}", node_text(key))
    }
}

fn decorate_source(value: &Node) -> String {
    let src = node_text(value);
    match value {
        Node::InterpolatedSymbolNode { .. } => format!(":\"{src}\""),
        Node::InterpolatedStringNode { .. } => format!("\"{src}\""),
        Node::StringNode { .. } if is_parser_dstr(value) => format!("\"{src}\""),
        Node::SymbolNode { .. } => format!(":{src}"),
        Node::StringNode { .. } => {
            let s = value.as_string_node().unwrap();
            let v = String::from_utf8_lossy(s.unescaped());
            format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        _ => format!("'{src}'"),
    }
}
