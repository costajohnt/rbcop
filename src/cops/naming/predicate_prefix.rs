use crate::prelude::*;

pub const NAME: &str = "Naming/PredicatePrefix";
pub static COP: PredicatePrefix = PredicatePrefix;
pub struct PredicatePrefix;

fn expected_name(cx: &Cx, name: &str, prefix: &str) -> String {
    let forbidden = cx.cfg.strings("ForbiddenPrefixes").iter().any(|p| p == prefix);
    let mut out = if forbidden { name.replacen(prefix, "", 1) } else { name.to_string() };
    if !name.ends_with('?') {
        out.push('?');
    }
    out
}

fn allowed_method_name(cx: &Cx, name: &str, prefix: &str) -> bool {
    // `/^#{prefix}[^0-9]/`: the prefix is interpolated unescaped
    let matches =
        name.starts_with(prefix) && crate::config::value::ruby_regex(&format!("^{prefix}[^0-9]")).is_some_and(|r| r.is_match(name));
    !matches
        || name == expected_name(cx, name, prefix)
        || name.ends_with('=')
        || helpers::allowed::allowed_method(cx.cfg, name)
}

fn message(cx: &Cx, name: &str, prefix: &str) -> String {
    format!("Rename `{name}` to `{}`.", expected_name(cx, name, prefix))
}

/// `(block (send nil? :sig) args (send _ :returns $_type))` with `type.source == 'T::Boolean'`
fn boolean_sig(node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if call.receiver().is_some() || call.arguments().is_some() || method_name(&call) != "sig" {
        return false;
    }
    let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return false };
    if block.parameters().is_some_and(|p| p.as_block_parameters_node().is_none()) {
        return false;
    }
    let Some(body) = block.body().and_then(|b| b.as_statements_node()) else { return false };
    let stmts: Vec<Node> = body.body().iter().collect();
    let [ret] = stmts.as_slice() else { return false };
    let Some(ret) = ret.as_call_node() else { return false };
    if method_name(&ret) != "returns" || ret.block().is_some() {
        return false;
    }
    matches!(call_args(&ret).as_slice(), [t] if node_text(t) == "T::Boolean")
}

/// `sorbet_sig?(node, return_type: 'T::Boolean')` on the def's parser left sibling.
fn sorbet_sig(cx: &Cx, node: &Node) -> bool {
    let Some(stmts) = cx.ancestors.last().and_then(|p| p.as_statements_node()) else { return false };
    let body: Vec<Node> = stmts.body().iter().collect();
    let start = node.location().start_offset();
    match body.iter().position(|n| n.location().start_offset() == start) {
        Some(i) if i > 0 => boolean_sig(&body[i - 1]),
        _ => false,
    }
}

impl Cop for PredicatePrefix {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let prefixes = cx.cfg.strings("NamePrefix");
        if let Some(call) = node.as_call_node() {
            let name = method_name(&call);
            if call.receiver().is_some() || !cx.cfg.strings("MethodDefinitionMacros").iter().any(|m| m == name) {
                return;
            }
            let Some(first) = call_args(&call).into_iter().next() else { return };
            let Some(sym) = first.as_symbol_node() else { return };
            let name = String::from_utf8_lossy(sym.unescaped()).into_owned();
            let (s, e) = (first.location().start_offset(), first.location().end_offset());
            for prefix in &prefixes {
                if !allowed_method_name(cx, &name, prefix) {
                    let msg = message(cx, &name, prefix);
                    cx.add(s, e, msg);
                }
            }
            return;
        }
        let def = node.as_def_node().unwrap();
        let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        let sorbet = cx.cfg.bool("UseSorbetSigs").unwrap_or(false);
        let loc = def.name_loc();
        for prefix in &prefixes {
            if allowed_method_name(cx, &name, prefix) || (sorbet && !sorbet_sig(cx, node)) {
                continue;
            }
            let msg = message(cx, &name, prefix);
            cx.add(loc.start_offset(), loc.end_offset(), msg);
        }
    }
}
