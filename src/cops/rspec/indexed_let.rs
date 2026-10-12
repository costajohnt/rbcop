use crate::prelude::*;

pub const NAME: &str = "RSpec/IndexedLet";
pub static COP: IndexedLet = IndexedLet;
pub struct IndexedLet;

static SUFFIX_INDEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)_?[0-9]+$").unwrap());
static INDEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]+").unwrap());

/// let_name: `{(block (send nil? #Helpers.all ({str sym} $_) ...) ...) (send nil? #Helpers.all ({str sym} $_) block_pass)}`
fn let_name(cx: &Cx, l: &helpers::rspec::Language, node: &Node) -> Option<String> {
    let call = node.as_call_node()?;
    if call.receiver().is_some() || !l.helper(method_name(&call)) {
        return None;
    }
    let args = send_args(&call);
    match helpers::rspec::block_of(&call) {
        Some(b) if !helpers::rspec::is_plain(&b) => return None,
        Some(_) => {}
        None if args.len() == 2 && call.block().is_some() => {}
        None => return None,
    }
    let first = args.first()?;
    if helpers::rspec::is_sym(cx, first) {
        return helpers::rails::sym_value(first);
    }
    helpers::rspec::is_str(first).then(|| String::from_utf8_lossy(first.as_string_node().unwrap().unescaped()).into_owned())
}

/// A list setting as `Array(...)` of the config value.
fn list(v: Option<&Value>) -> Vec<Value> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Seq(items)) => items.to_vec(),
        Some(v) => vec![v.clone()],
    }
}

impl Cop for IndexedLet {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if helpers::rspec::plain_block_call(node).is_none() || !helpers::rspec::spec_group(&l, node) {
            return;
        }
        let body = helpers::rspec::block_body(&node.as_call_node().unwrap());
        // `node.body.child_nodes` of a single statement are its parts: never a `let`
        if !helpers::rspec::is_begin_body(&body) {
            return;
        }
        let variable_number = cx.config.for_cop("Naming/VariableNumber");
        let identifiers: Vec<String> = list(variable_number.get("AllowedIdentifiers"))
            .into_iter()
            .chain(list(cx.cfg.get("AllowedIdentifiers")))
            .filter_map(|v| v.as_string())
            .collect();
        let patterns: Vec<crate::config::value::RubyRegex> = list(variable_number.get("AllowedPatterns"))
            .into_iter()
            .chain(list(cx.cfg.get("AllowedPatterns")))
            .filter_map(|v| match v {
                Value::Regex(r) => crate::config::value::ruby_regex(&r),
                v => v.as_string().and_then(|s| crate::config::value::ruby_regex(&s)),
            })
            .collect();
        let max = cx.cfg.int("Max").unwrap_or(1);
        // filter_indexed_lets: group by name without digits, groups over Max
        let mut groups: Vec<(String, Vec<(usize, usize, String)>)> = Vec::new();
        for stmt in helpers::rspec::body_children(body) {
            if !helpers::rspec::let_(&l, &stmt) {
                continue;
            }
            let Some(name) = let_name(cx, &l, &stmt) else { continue };
            if !SUFFIX_INDEX.is_match(&name) || identifiers.contains(&name) || patterns.iter().any(|p| p.is_match(&name)) {
                continue;
            }
            let key = INDEX.replace_all(&name, "").into_owned();
            let (s, e) = helpers::rspec::node_range(&stmt);
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push((s, e, name)),
                None => groups.push((key, vec![(s, e, name)])),
            }
        }
        for (_, lets) in groups {
            if lets.len() as i64 <= max {
                continue;
            }
            for (s, e, name) in lets {
                let index = INDEX.find(&name).map_or("", |m| m.as_str()).to_string();
                cx.add(s, e, format!("This `let` statement uses `{index}` in its name. Please give it a meaningful name."));
            }
        }
    }
}
