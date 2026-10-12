use crate::prelude::*;

pub const NAME: &str = "Rails/MatchRoute";
pub static COP: MatchRoute = MatchRoute;
pub struct MatchRoute;

const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

/// `key.value` of a pair key: None when it raises (no `value`) - the whole cop run is lost then.
fn key_value(k: &Node) -> Option<Option<String>> {
    if let Some(v) = helpers::rails::sym_value(k) {
        return Some(Some(format!(":{v}")));
    }
    if k.as_string_node().is_some() || k.as_interpolated_string_node().is_some() {
        return Some(None);
    }
    if k.as_integer_node().is_some() || k.as_float_node().is_some() || k.as_rational_node().is_some() || k.as_imaginary_node().is_some() {
        return Some(None);
    }
    None
}

/// via_pair: the first pair whose key value is `:via`. Err when a key's `value` raises.
fn via_pair<'pr>(hash: &Node<'pr>) -> Result<Option<ruby_prism::AssocNode<'pr>>, ()> {
    for p in helpers::rails::hash_pairs(hash) {
        match key_value(&p.key()) {
            None => return Err(()),
            Some(Some(v)) if v == ":via" => return Ok(Some(p)),
            _ => {}
        }
    }
    Ok(None)
}

/// extract_via: the via values (`.value` of each), Err when one raises.
fn extract_via(hash: &Node) -> Result<Vec<String>, ()> {
    let Some(pair) = via_pair(hash)? else { return Ok(vec!["get".to_string()]) };
    let via = pair.value();
    let value = |n: &Node| -> Result<String, ()> { helpers::rails::str_or_sym(n).ok_or(()) };
    if let Some(a) = via.as_array_node() {
        return a.elements().iter().map(|e| value(&e)).collect();
    }
    if helpers::literal_node::basic_literal(&via) {
        return Ok(vec![value(&via)?]);
    }
    Ok(Vec::new())
}

impl Cop for MatchRoute {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :match $_ $(hash ...) ?)
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "match") {
            return;
        }
        let args = send_args(&call);
        let is_hash = |n: &Node| helpers::rails::hash_elements(n).is_some();
        let (path, options) = match args.as_slice() {
            [p] => (p, None),
            [p, o] if is_hash(o) => (p, Some(o)),
            _ => return,
        };
        if !helpers::rails::within_routes(cx.ancestors) {
            return;
        }
        let opts = if is_hash(path) { Some(path) } else { options };
        let method = match opts {
            None => "get".to_string(),
            Some(o) => {
                let Ok(via) = extract_via(o) else { return };
                if via.len() != 1 || !HTTP_METHODS.contains(&via[0].as_str()) {
                    return;
                }
                via[0].clone()
            }
        };
        // replacement
        let src = cx.src;
        let method_and_rest = |h: &Node| -> Option<(String, Vec<String>)> {
            let via = via_pair(h).ok()?;
            let m = extract_via(h).ok()?.first()?.clone();
            let range = |p: &ruby_prism::AssocNode| (p.location().start_offset(), p.location().end_offset());
            let rest = helpers::rails::hash_pairs(h)
                .iter()
                .filter(|p| via.as_ref().is_none_or(|v| !helpers::node_equality::same(src, range(p), range(v))))
                .map(|p| node_text(&p.as_node()).to_string())
                .collect();
            Some((m, rest))
        };
        let replacement = if is_hash(path) {
            let Some((m, rest)) = method_and_rest(path) else { return };
            format!("{m} {}", rest.join(", "))
        } else if let Some(o) = options {
            let Some((m, rest)) = method_and_rest(o) else { return };
            if rest.is_empty() { format!("{m} {}", node_text(path)) } else { format!("{m} {}, {}", node_text(path), rest.join(", ")) }
        } else {
            format!("get {}", node_text(path))
        };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, format!("Use `{method}` instead of `match` to define a route."), |c| c.replace(s, e, &replacement));
    }
}
