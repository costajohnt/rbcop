use crate::helpers::did_you_mean;
use crate::prelude::*;
use crate::project_index::{self, Decl, Ix, ProjectIndex};
use std::collections::HashSet;

pub const NAME: &str = "Lint/NameTypo";
pub static COP: NameTypo = NameTypo;
pub struct NameTypo;

static METHOD_MEMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#([a-zA-Z_][a-zA-Z0-9_]*[?!]?)\(\)\z").unwrap());

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~", "+@",
    "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

#[derive(Default)]
struct State {
    literal_names: Option<HashSet<String>>,
}

/// Only with `AllCops/UseProjectIndex`: `Ns::Name` and `Const.method` that the index knows do
/// not exist, when a close spelling does.
impl Cop for NameTypo {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CONSTANT_PATH_NODE,
            kind::CALL_NODE,
            kind::CALL_TARGET_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::INDEX_TARGET_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(index) = project_index::get() else { return };
        if let Some(send) = project_index::parser_send(node) {
            return on_send(cx, index, send);
        }
        if !check(cx, "CheckConstants") {
            return;
        }
        let path = node.as_constant_path_node().unwrap();
        let Some(ns) = path.parent() else { return };
        if ns.as_constant_read_node().is_none() && ns.as_constant_path_node().is_none() {
            return;
        }
        let Some(short) = path.name().map(|n| String::from_utf8_lossy(n.as_slice()).into_owned()) else { return };
        let start = node.location().start_offset();
        let target = cx.parent().is_some_and(|p| match p {
            Node::ConstantPathWriteNode { .. } => p.as_constant_path_write_node().unwrap().target().location().start_offset() == start,
            Node::ConstantPathOrWriteNode { .. } => p.as_constant_path_or_write_node().unwrap().target().location().start_offset() == start,
            Node::ConstantPathAndWriteNode { .. } => p.as_constant_path_and_write_node().unwrap().target().location().start_offset() == start,
            Node::ConstantPathOperatorWriteNode { .. } => {
                p.as_constant_path_operator_write_node().unwrap().target().location().start_offset() == start
            }
            _ => false,
        });
        if target
            || allowed_name(cx, &short)
            || helpers::parser_parent::defines_module(cx.ancestors)
            || defined_check(cx.ancestors)
        {
            return;
        }
        // constant_typo_suggestion
        let Some(decl) = project_index::resolve_const_node(index, &ns, cx.ancestors) else { return };
        let dictionary = {
            let ix = index.read();
            if !ix.is_namespace(decl) || ix.find_member(decl, &short).is_some() || !complete_members(cx, &ix, decl) {
                return;
            }
            ix.members(decl)
                .into_iter()
                .filter_map(|m| {
                    let name = ix.name(m);
                    (!name.contains('#') && !name.contains('<')).then(|| name.rsplit("::").next().unwrap_or(name).to_string())
                })
                .collect::<Vec<String>>()
        };
        if literal_names(cx).contains(&short) {
            return;
        }
        let Some(suggestion) = spell_check(&short, dictionary) else { return };
        let msg = format!(
            "Possible typo: `{short}` is not defined in `{}`. Did you mean `{suggestion}`?",
            helpers::const_name::const_name(&ns).unwrap_or_default()
        );
        let l = path.name_loc();
        cx.add(l.start_offset(), l.end_offset(), msg);
    }
}

fn on_send(cx: &mut Cx, index: &ProjectIndex, send: project_index::Send) {
    if !check(cx, "CheckMethods") {
        return;
    }
    let Some(recv) = send.receiver else { return };
    if recv.as_constant_read_node().is_none() && recv.as_constant_path_node().is_none() {
        return;
    }
    let name = send.name;
    if allowed_name(cx, &name) || defined_check(cx.ancestors) {
        return;
    }
    let Some(sel) = send.selector else { return };
    // method_typo_suggestion
    let setter = name.ends_with('=')
        && !matches!(name.as_str(), "==" | "===" | "!=" | "<=" | ">=")
        && !OPERATOR_METHODS.contains(&name.as_str());
    let base = if setter { name.strip_suffix('=').unwrap().to_string() } else { name.clone() };
    let Some(decl) = project_index::resolve_const_node(index, &recv, cx.ancestors) else { return };
    let dictionary = {
        let ix = index.read();
        if !ix.is_namespace(decl) {
            return;
        }
        // responds_in_index?
        let mut candidates = vec![name.clone()];
        if base != name {
            candidates.push(base.clone());
        }
        if candidates.iter().any(|c| {
            let member = format!("{c}()");
            ix.singleton_member(decl, &member).is_some() || ix.find_member(decl, &member).is_some()
        }) {
            return;
        }
        if !complete_members(cx, &ix, decl) {
            return;
        }
        // method_member_names
        let mut scopes: Vec<Decl> = ix.ancestors(decl).into_iter().filter_map(|a| ix.singleton_of(a)).collect();
        scopes.push(decl);
        scopes
            .into_iter()
            .flat_map(|s| ix.members(s))
            .filter_map(|m| METHOD_MEMBER.captures(ix.name(m)).map(|c| c[1].to_string()))
            .collect::<Vec<String>>()
    };
    if literal_names(cx).contains(&base) {
        return;
    }
    let Some(suggestion) = spell_check(&base, dictionary) else { return };
    let suggestion = if setter { format!("{suggestion}=") } else { suggestion };
    let msg = format!(
        "Possible typo: `{}` does not respond to `{name}`. Did you mean `{suggestion}`?",
        helpers::const_name::const_name(&recv).unwrap_or_default()
    );
    cx.add(sel.0, sel.1, msg);
}

/// `check?(key)`: `cop_config.fetch(key, true)`.
fn check(cx: &Cx, key: &str) -> bool {
    cx.cfg.get(key).is_none_or(|v| !matches!(v, Value::Bool(false) | Value::Null))
}

fn allowed_name(cx: &Cx, name: &str) -> bool {
    cx.cfg.array("AllowedNames").iter().any(|n| n == name)
}

/// `defined_check?`: inside `defined?(...)`.
fn defined_check(anc: &[Node]) -> bool {
    anc.iter().any(|a| a.as_defined_node().is_some())
}

/// `complete_index_members?`: the ancestry resolves and the namespace is not a bundled gem's.
fn complete_members(cx: &Cx, ix: &Ix, decl: Decl) -> bool {
    ix.fully_resolved_ancestry(decl, false) && !gem_owned_namespace(cx, ix.name(decl))
}

/// `gem_owned_namespace?`.
fn gem_owned_namespace(cx: &Cx, name: &str) -> bool {
    if cx.config.all_cops().truthy("ProjectIndexIncludesGems") {
        return false;
    }
    let root = name.split("::").next().unwrap_or("").to_lowercase();
    let path_gems = cx.config.path_sourced_gems_in_target();
    cx.config.gem_versions_in_target().is_some_and(|gems| {
        gems.keys()
            .filter(|g| !path_gems.contains(g))
            .any(|g| g.chars().filter(|c| *c != '-' && *c != '_').collect::<String>().to_lowercase() == root)
    })
}

fn literal_names<'a>(cx: &'a mut Cx) -> &'a HashSet<String> {
    if cx.state::<State>().literal_names.is_none() {
        let names = cx.ancestors.first().map(|root| project_index::literals(cx.src, root, false).names).unwrap_or_default();
        cx.state::<State>().literal_names = Some(names);
    }
    cx.state::<State>().literal_names.as_ref().unwrap()
}

/// `spell_check`: DidYouMean::SpellChecker's first correction from the de-duplicated dictionary.
fn spell_check(name: &str, mut dictionary: Vec<String>) -> Option<String> {
    let mut seen = HashSet::new();
    dictionary.retain(|w| seen.insert(w.clone()));
    let words: Vec<&str> = dictionary.iter().map(String::as_str).collect();
    did_you_mean::correct(name, &words).first().map(|s| s.to_string())
}
