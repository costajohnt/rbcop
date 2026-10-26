use crate::prelude::*;
use helpers::literal_node::expression_range;
use helpers::ruby_string::symbol_inspect;

pub const NAME: &str = "Lint/SymbolConversion";
pub static COP: SymbolConversion = SymbolConversion;
pub struct SymbolConversion;

impl Cop for SymbolConversion {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SYMBOL_NODE, kind::HASH_NODE, kind::KEYWORD_HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let strict = cx.cfg.str("EnforcedStyle") != Some("consistent");
        match node {
            Node::CallNode { .. } => on_send(cx, &node.as_call_node().unwrap()),
            Node::SymbolNode { .. } => on_sym(cx, node, strict),
            _ if !strict => {
                let keys = hash_keys(node);
                if keys.iter().any(requires_quotes) {
                    for key in &keys {
                        if requires_quotes(key) {
                            continue;
                        }
                        // `{foo:, ...}` cannot quote its key without a value (1.92).
                        let omitted = elements(node).iter().filter_map(|e| e.as_assoc_node()).any(|a| {
                            a.key().location().start_offset() == key.location().start_offset()
                                && a.value().as_implicit_node().is_some()
                        });
                        if omitted {
                            continue;
                        }
                        let value = String::from_utf8_lossy(key.as_symbol_node().unwrap().unescaped()).into_owned();
                        let correction = format!("\"{value}\"");
                        if properly_quoted(strict, source(cx, key), &correction) {
                            continue;
                        }
                        let msg = format!(
                            "Symbol hash key should be quoted for consistency; use `{correction}:` instead."
                        );
                        register(cx, key, &correction, msg);
                    }
                } else {
                    for key in &keys {
                        correct_hash_key(cx, key, node_colon(node, key), strict);
                    }
                }
            }
            _ => {}
        }
    }
}

fn register(cx: &mut Cx, node: &Node, correction: &str, msg: String) {
    let (s, e) = expression_range(node);
    cx.add_fix(s, e, msg, |c| c.replace(s, e, correction));
}

fn message(correction: &str) -> String {
    format!("Unnecessary symbol conversion; use `{correction}` instead.")
}

fn source<'a>(cx: &Cx<'a, '_>, node: &Node) -> &'a str {
    let (s, e) = expression_range(node);
    &cx.src.text[s..e]
}

fn on_send(cx: &mut Cx, call: &ruby_prism::CallNode) {
    // RESTRICT_ON_SEND to_sym/intern, no on_csend.
    if !matches!(method_name(call), "to_sym" | "intern") || is_csend(call) {
        return;
    }
    let Some(recv) = call.receiver() else { return };
    let correction = if let Some(s) = recv.as_symbol_node() {
        symbol_inspect(s.unescaped())
    } else if is_parser_dstr(&recv) {
        if is_heredoc(&recv) {
            return;
        }
        let opening = match recv.as_interpolated_string_node() {
            Some(i) => i.opening_loc(),
            None => recv.as_string_node().unwrap().opening_loc(),
        };
        if opening.is_some_and(|o| o.as_slice() == b"\"") {
            let src = node_text(&recv);
            format!(":\"{}\"", &src[1..src.len() - 1])
        } else {
            format!(":\"{}\"", String::from_utf8_lossy(&dstr_value(&recv)))
        }
    } else if let Some(s) = recv.as_string_node() {
        symbol_inspect(s.unescaped())
    } else {
        return;
    };
    let (s, e) = send_range(call);
    cx.add_fix(s, e, message(&correction), |c| c.replace(s, e, &correction));
}

/// rubocop-ast `DstrNode#value`: str children's values, other children's source.
fn dstr_value(node: &Node) -> Vec<u8> {
    if let Some(s) = node.as_string_node() {
        return s.unescaped().to_vec();
    }
    let Some(i) = node.as_interpolated_string_node() else { return node_text(node).as_bytes().to_vec() };
    let mut out = Vec::new();
    for part in i.parts().iter() {
        match part {
            Node::StringNode { .. } | Node::InterpolatedStringNode { .. } => out.extend(dstr_value(&part)),
            // `#@x` is a bare `ivar` child in parser: its source lacks the `#`.
            Node::EmbeddedVariableNode { .. } => {
                out.extend(node_text(&part.as_embedded_variable_node().unwrap().variable()).as_bytes())
            }
            _ => out.extend(node_text(&part).as_bytes()),
        }
    }
    out
}

fn on_sym(cx: &mut Cx, node: &Node, strict: bool) {
    let sym = node.as_symbol_node().unwrap();
    // A multi-line quoted symbol is a parser `dsym`.
    // ponytail: ignores backslash-newline continuations (see ast::is_parser_dstr) in `:"..."`.
    if sym.value_loc().is_some_and(|v| {
        let t = text(&v);
        t.strip_suffix('\n').unwrap_or(t).contains('\n')
    }) {
        return;
    }
    let parent = cx.ancestors.last();
    let grand = cx.ancestors.len().checked_sub(2).map(|i| &cx.ancestors[i]);
    // ignored_node?: keys of a consistent-style hash that has a key requiring quotes.
    if !strict
        && let (Some(p), Some(g)) = (parent.and_then(|p| p.as_assoc_node()), grand)
        && same(&p.key(), node)
        && hash_keys(g).iter().any(requires_quotes)
    {
        return;
    }
    if properly_quoted(strict, source(cx, node), &symbol_inspect(sym.unescaped())) {
        return;
    }
    if parent.is_some_and(|p| p.as_alias_method_node().is_some()) {
        return;
    }
    if parent.and_then(|p| p.as_array_node()).is_some_and(|a| {
        a.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%i") || o.as_slice().starts_with(b"%I"))
    }) {
        return;
    }
    // hash_key?: structural `==` with the pair's key (so `{a: :"a"}`'s value counts too).
    if let Some(p) = parent.and_then(|p| p.as_assoc_node()) {
        let key = p.key();
        if key.as_symbol_node().is_some_and(|k| k.unescaped() == sym.unescaped()) {
            let colon = p.operator_loc().is_none();
            correct_hash_key(cx, node, colon, strict);
            return;
        }
    }
    let correction = symbol_inspect(sym.unescaped());
    register(cx, node, &correction, message(&correction));
}

fn same(a: &Node, b: &Node) -> bool {
    a.location().start_offset() == b.location().start_offset() && a.location().end_offset() == b.location().end_offset()
}

fn correct_hash_key(cx: &mut Cx, node: &Node, colon: bool, strict: bool) {
    let sym = node.as_symbol_node().unwrap();
    let value = sym.unescaped();
    if !value.first().is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') {
        return;
    }
    let mut correction = symbol_inspect(value);
    if colon {
        correction.remove(0);
    }
    if properly_quoted(strict, source(cx, node), &correction) {
        return;
    }
    let shown = if colon { format!("{correction}:") } else { correction.clone() };
    register(cx, node, &correction, message(&shown));
}

/// The pair owning `key` in `hash` uses `key: value` syntax.
fn node_colon(hash: &Node, key: &Node) -> bool {
    elements(hash)
        .into_iter()
        .filter_map(|e| e.as_assoc_node())
        .find(|a| same(&a.key(), key))
        .is_some_and(|a| a.operator_loc().is_none())
}

fn elements<'pr>(hash: &Node<'pr>) -> Vec<Node<'pr>> {
    match hash {
        Node::HashNode { .. } => hash.as_hash_node().unwrap().elements().iter().collect(),
        Node::KeywordHashNode { .. } => hash.as_keyword_hash_node().unwrap().elements().iter().collect(),
        _ => Vec::new(),
    }
}

/// `node.keys.select(&:sym_type?)`
fn hash_keys<'pr>(hash: &Node<'pr>) -> Vec<Node<'pr>> {
    elements(hash)
        .into_iter()
        .filter_map(|e| e.as_assoc_node().map(|a| a.key()))
        .filter(|k| k.as_symbol_node().is_some())
        .collect()
}

/// `value.inspect.match?(/^:".*?"|=$/)`
fn requires_quotes(key: &Node) -> bool {
    let inspect = symbol_inspect(key.as_symbol_node().unwrap().unescaped());
    inspect.starts_with(":\"") || inspect.ends_with('=')
}

fn properly_quoted(strict: bool, source: &str, value: &str) -> bool {
    if strict && (!source.contains(['\'', '"']) || value.ends_with('=')) {
        return true;
    }
    source == value || source.replace('"', "\\\"").replace('\'', "\"") == value
}
