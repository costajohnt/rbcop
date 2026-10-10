use crate::config::{Pattern, Resolved, glob};
use crate::config::value::ruby_regex;
use crate::prelude::*;
use ruby_prism::Visit;
use std::path::Path;

pub const NAME: &str = "Naming/FileName";
pub static COP: FileName = FileName;
pub struct FileName;

static SNAKE_CASE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[0-9\p{Lowercase}_.?!]+$").unwrap());

impl Cop for FileName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, program: &Node) {
        let file = crate::config::expand_path(Path::new(cx.src.path), &crate::config::cwd());
        if cx.config.file_to_exclude(&file) || allowed_camel_case_file(cx.config, &file) {
            return;
        }
        let basename = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let regex = cx.cfg.get("Regex").and_then(|v| v.as_string());
        let msg = if filename_good(&basename, regex.as_deref()) {
            if !cx.cfg.bool("ExpectMatchingDefinition").unwrap_or(false) {
                return;
            }
            let roots = cx.cfg.strings("CheckDefinitionPathHierarchyRoots");
            let acronyms = cx.cfg.strings("AllowedAcronyms");
            let finder = Finder { acronyms: &acronyms };
            let path = file.to_string_lossy();
            if cx.cfg.bool("CheckDefinitionPathHierarchy").unwrap_or(false)
                && !finder.find(program, to_namespace(&path, &roots))
            {
                no_definition_message(&basename, &to_namespace(&path, &roots))
            } else if !finder.find(program, to_namespace(&basename, &roots)) {
                no_definition_message(&basename, &to_namespace(&basename, &roots))
            } else {
                return;
            }
        } else {
            if cx.cfg.bool("IgnoreExecutableScripts").unwrap_or(false) && cx.src.text.starts_with("#!") {
                return;
            }
            match &regex {
                Some(r) => format!("`{basename}` should match `{r}`."),
                None => format!("The name of this source file (`{basename}`) should use snake_case."),
            }
        };
        cx.add_global(msg);
    }
}

fn no_definition_message(basename: &str, namespace: &[String]) -> String {
    format!("`{basename}` should define a class or module called `{}`.", namespace.join("::"))
}

fn filename_good(basename: &str, regex: Option<&str>) -> bool {
    let b = basename.strip_prefix('.').unwrap_or(basename);
    let b = match b.rfind('.') {
        Some(i) if i + 1 < b.len() => &b[..i],
        _ => b,
    };
    let b = b.replacen('+', "_", 1);
    match regex {
        Some(r) => ruby_regex(r).is_some_and(|re| re.is_match(&b)),
        None => SNAKE_CASE.is_match(&b),
    }
}

/// Config#allowed_camel_case_file?: gemspecs, and files matched by an AllCops/Include pattern
/// containing an uppercase letter (Config#file_to_include? restricted to those patterns).
fn allowed_camel_case_file(config: &Resolved, file: &Path) -> bool {
    if file.extension().is_some_and(|e| e == "gemspec") {
        return true;
    }
    let all = config.all_cops();
    let Some(Value::Seq(items)) = all.get("Include") else { return false };
    let include: Vec<(Pattern, String)> = items
        .iter()
        .filter_map(|v| match v {
            Value::Regex(r) => ruby_regex(r).map(|re| (Pattern::Regex(re), r.clone())),
            v => {
                let s = v.as_string()?;
                let p = if glob::is_glob(&s) { Pattern::Glob(s.clone()) } else { Pattern::Exact(s.clone()) };
                Some((p, s))
            }
        })
        .collect();
    let rel = config.relative(file);
    let abs = file.display().to_string();
    let hidden = rel.starts_with('.') && !rel.starts_with("..");
    let possibly_hidden = include.iter().any(|(p, s)| match p {
        Pattern::Regex(_) => true,
        _ => s.starts_with('.') || s.contains("/."),
    });
    if hidden && !possibly_hidden {
        return false;
    }
    include.iter().filter(|(_, s)| s.chars().any(|c| c.is_ascii_uppercase())).any(|(p, s)| {
        let absolute_or_parent = !matches!(p, Pattern::Regex(_)) && (s.starts_with('/') || s.starts_with(".."));
        p.matches(if absolute_or_parent || rel.starts_with("..") { &abs } else { &rel })
    })
}

/// to_namespace: path components after the last hierarchy root, as module names.
fn to_namespace(path: &str, roots: &[String]) -> Vec<String> {
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    match components.iter().rposition(|c| roots.iter().any(|r| r == c)) {
        Some(i) => components[i + 1..].iter().map(|c| to_module_name(c)).collect(),
        None => components.last().map(|c| vec![to_module_name(c)]).unwrap_or_default(),
    }
}

fn to_module_name(basename: &str) -> String {
    let stem = basename.split('.').next().unwrap_or("");
    stem.split('_').map(capitalize).collect()
}

/// Ruby String#capitalize.
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

/// The parser `const` a definition node defines: (namespace node, name).
/// A const's namespace: none, `::` (cbase) or a node.
enum Ns<'pr> {
    Nil,
    Cbase,
    Node(Node<'pr>),
}

fn ns_of<'pr>(parent: Option<Node<'pr>>, cbase: bool) -> Ns<'pr> {
    match parent {
        Some(p) => Ns::Node(p),
        None if cbase => Ns::Cbase,
        None => Ns::Nil,
    }
}

fn defined_module<'pr>(node: &Node<'pr>, with_struct: bool) -> Option<(Ns<'pr>, String)> {
    let path = match node {
        Node::ClassNode { .. } => node.as_class_node()?.constant_path(),
        Node::ModuleNode { .. } => node.as_module_node()?.constant_path(),
        Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. } => {
            let (ns, name, value) = match node.as_constant_write_node() {
                Some(w) => (Ns::Nil, w.name().as_slice().to_vec(), w.value()),
                None => {
                    let w = node.as_constant_path_write_node()?;
                    let t = w.target();
                    (ns_of(t.parent(), true), t.name()?.as_slice().to_vec(), w.value())
                }
            };
            let call = value.as_call_node()?;
            let ok = |recv: &str, meth: &str| {
                method_name(&call) == meth && call.receiver().is_some_and(|r| global_const(&r, recv))
            };
            let plain_block = call.block().is_none_or(|b| {
                b.as_block_node().is_some_and(|b| {
                    !matches!(b.parameters(), Some(Node::NumberedParametersNode { .. } | Node::ItParametersNode { .. }))
                }) || b.as_block_argument_node().is_some()
            });
            let class_new = ok("Class", "new") || ok("Module", "new");
            if !plain_block || !(class_new || (with_struct && ok("Struct", "new"))) {
                return None;
            }
            return Some((ns, String::from_utf8_lossy(&name).into_owned()));
        }
        _ => return None,
    };
    const_parts(&path)
}

/// `(const {nil? cbase} :Name)`
fn global_const(node: &Node, name: &str) -> bool {
    match node {
        Node::ConstantReadNode { .. } => node_text(node) == name,
        Node::ConstantPathNode { .. } => {
            let p = node.as_constant_path_node().unwrap();
            p.parent().is_none() && text(&p.name_loc()) == name
        }
        _ => false,
    }
}

/// `namespace, name = *const`
fn const_parts<'pr>(node: &Node<'pr>) -> Option<(Ns<'pr>, String)> {
    match node {
        Node::ConstantReadNode { .. } => Some((Ns::Nil, node_text(node).to_string())),
        Node::ConstantPathNode { .. } => {
            let p = node.as_constant_path_node()?;
            Some((ns_of(p.parent(), true), text(&p.name_loc()).to_string()))
        }
        _ => None,
    }
}

struct Finder<'a> {
    acronyms: &'a [String],
}

impl Finder<'_> {
    fn match_acronym(&self, expected: &str, name: &str) -> bool {
        let name = self.acronyms.iter().fold(name.to_string(), |r, a| r.replace(a.as_str(), &capitalize(a)));
        expected == name
    }

    /// find_class_or_module: some class/module/casgn defines the namespace's last name inside the rest.
    fn find(&self, program: &Node, mut namespace: Vec<String>) -> bool {
        let name = namespace.pop();
        let mut v = Defs { finder: self, name, namespace, stack: Vec::new(), found: false };
        v.visit(program);
        v.found
    }
}

struct Defs<'a, 'pr> {
    finder: &'a Finder<'a>,
    name: Option<String>,
    namespace: Vec<String>,
    /// enclosing class/module/sclass/casgn nodes (each_ancestor)
    stack: Vec<Node<'pr>>,
    found: bool,
}

impl<'pr> Defs<'_, 'pr> {
    fn check(&mut self, node: &Node<'pr>) {
        if self.found {
            return;
        }
        let Some((const_ns, const_name)) = defined_module(node, true) else { return };
        let name = self.name.clone().unwrap_or_default();
        if self.name.as_deref() != Some(const_name.as_str()) && !self.finder.match_acronym(&name, &const_name) {
            return;
        }
        if self.namespace.is_empty() || self.namespace_matches(const_ns) {
            self.found = true;
        }
    }

    fn matched(&self) -> bool {
        self.namespace.is_empty() || self.namespace == ["Object"]
    }

    /// partial_matcher!: pops matching names off `namespace` walking a const chain.
    fn partial(&mut self, mut ns: Ns<'pr>) -> bool {
        loop {
            let n = match ns {
                Ns::Nil => return false,
                Ns::Cbase => return self.matched(),
                Ns::Node(n) => n,
            };
            let (next, name) = match &n {
                Node::ConstantPathNode { .. } => {
                    let p = n.as_constant_path_node().unwrap();
                    (ns_of(p.parent(), true), text(&p.name_loc()).to_string())
                }
                Node::ConstantReadNode { .. } => (Ns::Nil, node_text(&n).to_string()),
                Node::CallNode { .. } => {
                    let c = n.as_call_node().unwrap();
                    (ns_of(c.receiver(), false), method_name(&c).to_string())
                }
                _ => return false,
            };
            if let Some(last) = self.namespace.last().cloned()
                && (name == last || self.finder.match_acronym(&last, &name))
            {
                self.namespace.pop();
            }
            ns = next;
        }
    }

    fn namespace_matches(&mut self, const_ns: Ns<'pr>) -> bool {
        self.partial(const_ns);
        let ancestors: Vec<Node<'pr>> = std::mem::take(&mut self.stack);
        let mut result = None;
        for a in ancestors.iter().rev() {
            if a.as_singleton_class_node().is_some() {
                result = Some(false);
                break;
            }
            if let Some((ns, name)) = defined_module(a, false) {
                // match_partial.call(ancestor.defined_module): the const itself, then its namespace
                if let Some(last) = self.namespace.last().cloned()
                    && (name == last || self.finder.match_acronym(&last, &name))
                {
                    self.namespace.pop();
                }
                self.partial(ns);
            }
        }
        self.stack = ancestors;
        result.unwrap_or_else(|| self.matched())
    }
}

impl<'pr> Visit<'pr> for Defs<'_, 'pr> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        self.check(&node.as_node());
        self.stack.push(node.as_node());
        ruby_prism::visit_class_node(self, node);
        self.stack.pop();
    }
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'pr>) {
        self.check(&node.as_node());
        self.stack.push(node.as_node());
        ruby_prism::visit_module_node(self, node);
        self.stack.pop();
    }
    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'pr>) {
        self.stack.push(node.as_node());
        ruby_prism::visit_singleton_class_node(self, node);
        self.stack.pop();
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'pr>) {
        self.check(&node.as_node());
        self.stack.push(node.as_node());
        ruby_prism::visit_constant_write_node(self, node);
        self.stack.pop();
    }
    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode<'pr>) {
        self.check(&node.as_node());
        self.stack.push(node.as_node());
        ruby_prism::visit_constant_path_write_node(self, node);
        self.stack.pop();
    }
}
