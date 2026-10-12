//! rubocop-ast `Node#to_s` (parser's `to_sexp`) for a Prism subtree: `(type child...)`, a node
//! child on its own line indented two spaces per level, any other child inline as its `inspect`.
// ponytail: covers the plain-method shapes (sends, variables, constants, literals, arrays, hashes,
// simple parameters); `None` for anything else. Extend the match when a cop needs another shape.

use crate::ast::{call_args, int_literal_value, is_csend, is_heredoc, is_parser_dstr, parameter_nodes};
use crate::helpers::ruby_string::{float_to_s, inspect, symbol_inspect};
use ruby_prism::Node;

enum S {
    Atom(String),
    Node(&'static str, Vec<S>),
}

fn atom(s: impl Into<String>) -> S {
    S::Atom(s.into())
}

fn sym(id: ruby_prism::ConstantId) -> S {
    atom(symbol_inspect(id.as_slice()))
}

fn opt(node: Option<Node>, depth: usize) -> Option<S> {
    match node {
        Some(n) => conv(&n, depth),
        None => Some(atom("nil")),
    }
}

/// A body: nil, one statement, or a `begin` of several.
fn body(node: Option<Node>, depth: usize) -> Option<S> {
    let Some(n) = node else { return Some(atom("nil")) };
    let Some(st) = n.as_statements_node() else { return conv(&n, depth) };
    let kids: Vec<Node> = st.body().iter().collect();
    match kids.len() {
        0 => Some(atom("nil")),
        1 => conv(&kids[0], depth),
        _ => Some(S::Node("begin", kids.iter().map(|k| conv(k, depth)).collect::<Option<_>>()?)),
    }
}

fn args(params: Option<ruby_prism::ParametersNode>, depth: usize) -> Option<S> {
    let Some(p) = params else { return Some(S::Node("args", Vec::new())) };
    let mut out = Vec::new();
    for n in parameter_nodes(&p) {
        out.push(match n {
            Node::RequiredParameterNode { .. } => S::Node("arg", vec![sym(n.as_required_parameter_node()?.name())]),
            Node::OptionalParameterNode { .. } => {
                let o = n.as_optional_parameter_node()?;
                S::Node("optarg", vec![sym(o.name()), conv(&o.value(), depth)?])
            }
            Node::RequiredKeywordParameterNode { .. } => {
                S::Node("kwarg", vec![sym(n.as_required_keyword_parameter_node()?.name())])
            }
            Node::OptionalKeywordParameterNode { .. } => {
                let o = n.as_optional_keyword_parameter_node()?;
                S::Node("kwoptarg", vec![sym(o.name()), conv(&o.value(), depth)?])
            }
            Node::RestParameterNode { .. } => S::Node("restarg", vec![sym(n.as_rest_parameter_node()?.name()?)]),
            Node::KeywordRestParameterNode { .. } => {
                S::Node("kwrestarg", vec![sym(n.as_keyword_rest_parameter_node()?.name()?)])
            }
            Node::BlockParameterNode { .. } => S::Node("blockarg", vec![sym(n.as_block_parameter_node()?.name()?)]),
            _ => return None,
        });
    }
    Some(S::Node("args", out))
}

fn conv(node: &Node, depth: usize) -> Option<S> {
    let depth = depth.checked_sub(1)?;
    if let Some(v) = int_literal_value(node) {
        return Some(S::Node("int", vec![atom(v.to_string())]));
    }
    let leaf = |t: &'static str| Some(S::Node(t, Vec::new()));
    let list = |t: &'static str, kids: Vec<Node>| Some(S::Node(t, kids.iter().map(|k| conv(k, depth)).collect::<Option<_>>()?));
    match node {
        Node::DefNode { .. } => {
            let d = node.as_def_node()?;
            let mut kids = Vec::new();
            let t = match d.receiver() {
                Some(r) => {
                    kids.push(conv(&r, depth)?);
                    "defs"
                }
                None => "def",
            };
            kids.push(sym(d.name()));
            kids.push(args(d.parameters(), depth)?);
            kids.push(body(d.body(), depth)?);
            Some(S::Node(t, kids))
        }
        Node::CallNode { .. } => {
            let c = node.as_call_node()?;
            if c.block().is_some() || matches!(c.name().as_slice(), b"[]" | b"[]=") {
                return None;
            }
            let mut kids = vec![opt(c.receiver(), depth)?, sym(c.name())];
            for a in call_args(&c) {
                if matches!(a, Node::KeywordHashNode { .. } | Node::SplatNode { .. } | Node::BlockArgumentNode { .. }) {
                    return None;
                }
                kids.push(conv(&a, depth)?);
            }
            Some(S::Node(if is_csend(&c) { "csend" } else { "send" }, kids))
        }
        Node::LocalVariableReadNode { .. } => {
            let name = node.as_local_variable_read_node()?.name();
            // `_1` below 2.7 is a send for the parser gem
            if matches!(name.as_slice(), [b'_', b'1'..=b'9']) {
                return None;
            }
            Some(S::Node("lvar", vec![sym(name)]))
        }
        Node::InstanceVariableReadNode { .. } => Some(S::Node("ivar", vec![sym(node.as_instance_variable_read_node()?.name())])),
        Node::ClassVariableReadNode { .. } => Some(S::Node("cvar", vec![sym(node.as_class_variable_read_node()?.name())])),
        Node::GlobalVariableReadNode { .. } => Some(S::Node("gvar", vec![sym(node.as_global_variable_read_node()?.name())])),
        Node::LocalVariableWriteNode { .. } => {
            let w = node.as_local_variable_write_node()?;
            Some(S::Node("lvasgn", vec![sym(w.name()), conv(&w.value(), depth)?]))
        }
        Node::InstanceVariableWriteNode { .. } => {
            let w = node.as_instance_variable_write_node()?;
            Some(S::Node("ivasgn", vec![sym(w.name()), conv(&w.value(), depth)?]))
        }
        Node::ConstantReadNode { .. } => {
            Some(S::Node("const", vec![atom("nil"), sym(node.as_constant_read_node()?.name())]))
        }
        Node::ConstantPathNode { .. } => {
            let p = node.as_constant_path_node()?;
            let scope = match p.parent() {
                Some(parent) => conv(&parent, depth)?,
                None => S::Node("cbase", Vec::new()),
            };
            Some(S::Node("const", vec![scope, sym(p.name()?)]))
        }
        Node::AndNode { .. } => {
            let a = node.as_and_node()?;
            list("and", vec![a.left(), a.right()])
        }
        Node::OrNode { .. } => {
            let o = node.as_or_node()?;
            list("or", vec![o.left(), o.right()])
        }
        Node::SelfNode { .. } => leaf("self"),
        Node::NilNode { .. } => leaf("nil"),
        Node::TrueNode { .. } => leaf("true"),
        Node::FalseNode { .. } => leaf("false"),
        Node::FloatNode { .. } => Some(S::Node("float", vec![atom(float_to_s(node.as_float_node()?.value()))])),
        Node::StringNode { .. } if !is_parser_dstr(node) && !is_heredoc(node) => {
            Some(S::Node("str", vec![atom(inspect(node.as_string_node()?.unescaped()))]))
        }
        Node::SymbolNode { .. } => {
            let s = node.as_symbol_node()?;
            // an empty quoted symbol is parser's `(dsym)`, a multi-line one a `dsym` of parts
            if s.opening_loc().is_some_and(|o| o.as_slice() != b":") || s.value_loc().is_none() {
                return None;
            }
            Some(S::Node("sym", vec![atom(symbol_inspect(s.unescaped()))]))
        }
        Node::ArrayNode { .. } => {
            let a = node.as_array_node()?;
            let kids: Vec<Node> = a.elements().iter().collect();
            if a.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%"))
                || kids.iter().any(|k| matches!(k, Node::KeywordHashNode { .. } | Node::SplatNode { .. }))
            {
                return None;
            }
            list("array", kids)
        }
        Node::HashNode { .. } => {
            let mut pairs = Vec::new();
            for e in node.as_hash_node()?.elements().iter() {
                let a = e.as_assoc_node()?;
                let key = match a.key().as_symbol_node() {
                    // a label: `a: 1` is `(sym :a)`; the quoted forms are left out
                    Some(k) if k.closing_loc().is_some() => {
                        if k.opening_loc().is_some() {
                            return None;
                        }
                        S::Node("sym", vec![atom(symbol_inspect(k.unescaped()))])
                    }
                    _ => conv(&a.key(), depth)?,
                };
                pairs.push(S::Node("pair", vec![key, conv(&a.value(), depth)?]));
            }
            Some(S::Node("hash", pairs))
        }
        Node::ReturnNode { .. } => {
            let r = node.as_return_node()?;
            let kids: Vec<Node> = r.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
            if kids.iter().any(|k| matches!(k, Node::KeywordHashNode { .. } | Node::SplatNode { .. })) {
                return None;
            }
            list("return", kids)
        }
        _ => None,
    }
}

fn print(s: &S, indent: usize, out: &mut String) {
    match s {
        S::Atom(a) => {
            out.push(' ');
            out.push_str(a);
        }
        S::Node(t, kids) => {
            out.push_str(&"  ".repeat(indent));
            out.push('(');
            out.push_str(t);
            for k in kids {
                if let S::Node(..) = k {
                    out.push('\n');
                    print(k, indent + 1, out);
                } else {
                    print(k, indent, out);
                }
            }
            out.push(')');
        }
    }
}

/// `node.to_s`, or None when the subtree holds a node this port does not print.
pub fn sexp(node: &Node) -> Option<String> {
    let s = conv(node, 200)?;
    let mut out = String::new();
    print(&s, 0, &mut out);
    Some(out)
}
