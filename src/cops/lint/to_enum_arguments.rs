use crate::prelude::*;

pub const NAME: &str = "Lint/ToEnumArguments";
pub static COP: ToEnumArguments = ToEnumArguments;
pub struct ToEnumArguments;

/// Parser type of a def parameter.
#[derive(PartialEq, Clone, Copy)]
enum Param {
    Arg,
    OptArg,
    RestArg,
    KwArg,
    KwRestArg,
    ForwardArg,
    BlockArg,
    Other,
}

fn param_type(n: &Node) -> Param {
    match kind::of(n) {
        kind::REQUIRED_PARAMETER_NODE => Param::Arg,
        kind::OPTIONAL_PARAMETER_NODE => Param::OptArg,
        kind::REST_PARAMETER_NODE => Param::RestArg,
        kind::REQUIRED_KEYWORD_PARAMETER_NODE | kind::OPTIONAL_KEYWORD_PARAMETER_NODE => Param::KwArg,
        kind::KEYWORD_REST_PARAMETER_NODE => Param::KwRestArg,
        kind::FORWARDING_PARAMETER_NODE => Param::ForwardArg,
        kind::BLOCK_PARAMETER_NODE => Param::BlockArg,
        _ => Param::Other,
    }
}

/// Parameter name (`children[0]`) as text.
fn param_name<'pr>(n: &Node<'pr>) -> &'pr [u8] {
    if let Some(p) = n.as_optional_parameter_node() {
        return p.name().as_slice();
    }
    if let Some(p) = n.as_required_keyword_parameter_node() {
        return p.name().as_slice();
    }
    if let Some(p) = n.as_optional_keyword_parameter_node() {
        return p.name().as_slice();
    }
    b""
}

/// Elements of an unbraced (keyword) hash argument, or of any hash when `braced` is allowed.
fn hash_elements<'pr>(n: &Node<'pr>, braced: bool) -> Option<Vec<Node<'pr>>> {
    if let Some(h) = n.as_keyword_hash_node() {
        return Some(h.elements().iter().collect());
    }
    n.as_hash_node().filter(|_| braced).map(|h| h.elements().iter().collect())
}

/// `kwsplat` / `forwarded_kwrestarg` hash children.
fn is_kwsplat(n: &Node) -> bool {
    n.as_assoc_splat_node().is_some()
}

/// `(pair (sym name) (lvar name))`
fn passing_keyword(pair: &Node, name: &[u8]) -> bool {
    let Some(p) = pair.as_assoc_node() else { return false };
    let key = p.key().as_symbol_node().is_some_and(|s| s.unescaped() == name);
    let mut v = p.value();
    if let Some(i) = v.as_implicit_node() {
        v = i.value();
    }
    key && v.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name)
}

fn argument_match(send_arg: &Node, def_arg: &Node) -> bool {
    match param_type(def_arg) {
        Param::Arg | Param::RestArg => node_text(send_arg) == node_text(def_arg),
        Param::OptArg => node_text(send_arg).as_bytes() == param_name(def_arg),
        Param::KwArg => hash_elements(send_arg, false)
            .is_some_and(|els| els.iter().any(|p| passing_keyword(p, param_name(def_arg)))),
        Param::KwRestArg => hash_elements(send_arg, false)
            .is_some_and(|els| els.iter().any(|c| is_kwsplat(c) && node_text(c) == node_text(def_arg))),
        Param::ForwardArg => send_arg.as_forwarding_arguments_node().is_some(),
        Param::BlockArg | Param::Other => false,
    }
}

fn arguments_match(args: &[Node], params: &[Node]) -> bool {
    let mut index = 0;
    let all_present = params.iter().filter(|p| param_type(p) != Param::BlockArg).all(|def_arg| {
        let send_arg = args.get(index);
        if matches!(param_type(def_arg), Param::Arg | Param::RestArg | Param::OptArg) {
            index += 1;
        }
        send_arg.is_some_and(|s| argument_match(s, def_arg))
    });
    all_present && !extra_positional(args, params) && !extra_keyword(args, params)
}

fn extra_positional(args: &[Node], params: &[Node]) -> bool {
    let variadic = params.iter().any(|p| matches!(param_type(p), Param::RestArg | Param::ForwardArg));
    // splat, forwarded_args, forwarded_restarg
    let expandable =
        args.iter().any(|a| a.as_splat_node().is_some() || a.as_forwarding_arguments_node().is_some());
    if variadic || expandable {
        return false;
    }
    let positional = args
        .iter()
        .filter(|a| {
            a.as_hash_node().is_none() && a.as_keyword_hash_node().is_none() && a.as_block_argument_node().is_none()
        })
        .count();
    positional > params.iter().filter(|p| matches!(param_type(p), Param::Arg | Param::OptArg)).count()
}

fn extra_keyword(args: &[Node], params: &[Node]) -> bool {
    if params.iter().any(|p| matches!(param_type(p), Param::KwRestArg | Param::ForwardArg)) {
        return false;
    }
    let expandable = args.iter().any(|a| {
        a.as_forwarding_arguments_node().is_some()
            || hash_elements(a, true).is_some_and(|els| els.iter().any(is_kwsplat))
    });
    if expandable {
        return false;
    }
    let keywords: Vec<&[u8]> =
        params.iter().filter(|p| param_type(p) == Param::KwArg).map(|p| param_name(p)).collect();
    args.iter().filter_map(|a| hash_elements(a, true)).flatten().any(|p| {
        p.as_assoc_node()
            .and_then(|p| p.key().as_symbol_node())
            .is_some_and(|s| !keywords.contains(&s.unescaped()))
    })
}

impl Cop for ToEnumArguments {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "to_enum" | "enum_for")
            || is_csend(&call)
            || !call.receiver().is_none_or(|r| r.as_self_node().is_some())
        {
            return;
        }
        let Some(def) = cx.ancestors.iter().rev().find_map(|a| a.as_def_node()) else { return };
        let args = send_args(&call);
        let Some((method_node, arguments)) = args.split_first() else { return };
        let def_name = def.name().as_slice();
        let names_method = if let Some(c) = method_node.as_call_node() {
            c.receiver().is_none()
                && c.arguments().is_none()
                && c.block().is_none()
                && matches!(method_name(&c), "__method__" | "__callee__")
        } else {
            method_node.as_symbol_node().is_some_and(|s| s.unescaped() == def_name)
        };
        if !names_method {
            return;
        }
        let params = def.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default();
        if arguments_match(arguments, &params) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Ensure you correctly provided all the arguments.");
    }
}
