//! `AllCops/UseProjectIndex` (#295): RuboCop's project-wide index (`ProjectIndexLoader`, built
//! with the rubydex gem) and the `ProjectIndexHelp` / `IndexedMethodArity` mixins its cops use.
//!
//! The index is vendor/rubydex, the gem's own Rust core, so declarations, ancestors and constant
//! resolution come out as RuboCop sees them. This module is the gem's Ruby API (rubydex-sys and
//! lib/rubydex) over it: one index per run, built before inspection, shared by every worker.

use ruby_prism::Node;
use rubydex::model::declaration::{Ancestor, Declaration, Namespace};
use rubydex::model::definitions::{Definition, Mixin, Parameter};
use rubydex::model::graph::Graph;
use rubydex::model::ids::{DeclarationId, StringId, declaration_id_from_lookup_name};
use rubydex::model::name::NameRef;
pub use rubydex::model::visibility::Visibility;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock, RwLockReadGuard};

pub use rubydex::model::ids::DeclarationId as Decl;
pub use rubydex::model::ids::DefinitionId as Def;

/// A constant name and the lexical nesting it is resolved in.
type ResolveKey = (String, Vec<String>);

pub struct ProjectIndex {
    graph: RwLock<Graph>,
    /// `Graph#resolve_constant` results: resolving adds (then untracks) names, so it takes the
    /// write lock; memoized because DeprecatedReference resolves every constant of every file.
    resolved: Mutex<HashMap<ResolveKey, Option<DeclarationId>>>,
    /// `Graph#method_references` names, `()` dropped (UnusedPrivateMethod#compute_referenced_names).
    method_references: RwLock<HashSet<String>>,
    /// ProjectIndexHelp#project_index_signature: `path:mtime:size` of every indexed document.
    signature: RwLock<String>,
}

/// One index for the process. `--lsp` and `--mcp` rebuild it in place, between requests, so no
/// cop holds a view of the old graph and `get` stays a plain load.
static INDEX: OnceLock<ProjectIndex> = OnceLock::new();

/// `Cop::Base#project_index`: None unless the run enabled `AllCops/UseProjectIndex`.
pub fn get() -> Option<&'static ProjectIndex> {
    INDEX.get()
}

/// ProjectIndexLoader.build_index: index `paths` (rubydex skips anything but .rb/.rake/.rbs/.ru
/// and files that are not UTF-8), then resolve. Installed for the rest of the process, replacing
/// any index built before.
pub fn install(paths: Vec<String>) {
    let mut graph = Graph::new();
    let (files, _) = rubydex::listing::collect_file_paths(paths, &graph.excluded_patterns());
    // Graph#index_all returns I/O errors, which RuboCop ignores.
    let _ = rubydex::indexing::index_files(&mut graph, files, rubydex::indexing::IndexerBackend::RubyIndexer);
    rubydex::resolution::Resolver::new(&mut graph).resolve();
    let mut lines: Vec<String> = graph
        .documents()
        .values()
        .filter_map(|doc| {
            let path = doc.uri().strip_prefix("file://")?;
            let (mtime, size) = std::fs::metadata(path)
                .ok()
                .map(|m| {
                    let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
                    (t.map_or(0, |d| d.as_nanos()), m.len())
                })
                .unwrap_or((0, 0));
            Some(format!("{path}:{mtime}:{size}"))
        })
        .collect();
    lines.sort();
    let method_references: HashSet<String> = graph
        .method_references()
        .values()
        .filter_map(|r| graph.strings().get(r.str()))
        .map(|s| s.as_str().strip_suffix("()").unwrap_or(s.as_str()).to_string())
        .collect();
    let signature = lines.join("\n");
    if let Some(index) = INDEX.get() {
        *index.graph.write().unwrap_or_else(|e| e.into_inner()) = graph;
        index.resolved.lock().unwrap_or_else(|e| e.into_inner()).clear();
        *index.method_references.write().unwrap_or_else(|e| e.into_inner()) = method_references;
        *index.signature.write().unwrap_or_else(|e| e.into_inner()) = signature;
        return;
    }
    let _ = INDEX.set(ProjectIndex {
        graph: RwLock::new(graph),
        resolved: Mutex::default(),
        method_references: RwLock::new(method_references),
        signature: RwLock::new(signature),
    });
}

/// ProjectIndexLoader.bundled_gem_source_files (`AllCops/ProjectIndexIncludesGems`): every bundled
/// gem's `lib/**/*.rb`, empty outside a bundle. The project's Ruby asks Bundler, so the bundle
/// resolves as inside RuboCop, and what Bundler prints (`Resolving dependencies...`) reaches
/// stdout as there. `Bundler.load` is memoized per process, so a rebuilt index (`--lsp`) reuses
/// the first list. Err: the message RuboCop warns with before indexing the project only.
// ponytail: the memoized list also skips re-globbing the gem dirs, which do not change in a run.
pub fn bundled_gem_source_files(cwd: &Path) -> Result<Vec<String>, String> {
    use std::io::Write;
    const MARK: &[u8] = b"\0rbcop-bundled-gems\0";
    const SCRIPT: &str = r##"out = begin
  require 'bundler'
  files = if Bundler::SharedHelpers.in_bundle?
    Bundler.load.specs.flat_map { |spec| Dir.glob(File.join(spec.full_gem_path, 'lib', '**', '*.rb')) }
  else
    []
  end
  "F" + files.map { |f| "#{f}\0" }.join
rescue LoadError
  "F"
rescue StandardError => e
  "E#{e.message}"
end
$stdout.binmode
$stdout.print "\0rbcop-bundled-gems\0", out
"##;
    static FILES: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(files) = FILES.get() {
        return Ok(files.clone());
    }
    let _ = std::io::stdout().flush();
    let out = std::process::Command::new("ruby")
        .args(["-e", SCRIPT])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run ruby: {e}"))?;
    let at = out.stdout.windows(MARK.len()).rposition(|w| w == MARK).ok_or("ruby exited without a result")?;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(&out.stdout[..at]);
    let _ = stdout.flush();
    match out.stdout[at + MARK.len()..].split_first() {
        Some((b'F', list)) => {
            let files: Vec<String> = list
                .split(|b| *b == 0)
                .filter(|f| !f.is_empty())
                .map(|f| String::from_utf8_lossy(f).into_owned())
                .collect();
            Ok(FILES.get_or_init(|| files).clone())
        }
        Some((_, message)) => Err(String::from_utf8_lossy(message).into_owned()),
        None => Err("ruby exited without a result".into()),
    }
}

impl ProjectIndex {
    /// What ProjectIndexHelp#external_dependency_checksum covers, for the result cache key.
    pub fn signature(&self) -> String {
        self.signature.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn read(&self) -> Ix<'_> {
        Ix { g: self.graph.read().unwrap_or_else(|e| e.into_inner()) }
    }

    /// `Graph#resolve_constant(name, nesting)`.
    pub fn resolve_constant(&self, name: &str, nesting: &[String]) -> Option<Decl> {
        let key = (name.to_string(), nesting.to_vec());
        if let Some(hit) = self.resolved.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return *hit;
        }
        let found = {
            let mut g = self.graph.write().unwrap_or_else(|e| e.into_inner());
            resolve_in(&mut g, name, nesting)
        };
        self.resolved.lock().unwrap_or_else(|e| e.into_inner()).insert(key, found);
        found
    }

    /// `name` is among `Graph#method_references` (UnusedPrivateMethod#compute_referenced_names).
    pub fn method_referenced(&self, name: &str) -> bool {
        self.method_references.read().unwrap_or_else(|e| e.into_inner()).contains(name)
    }
}

/// rdx_graph_resolve_constant (name_api::nesting_stack_to_name_id + Resolver#resolve_constant).
fn resolve_in(graph: &mut Graph, const_name: &str, nesting: &[String]) -> Option<Decl> {
    use rubydex::model::ids::NameId;
    use rubydex::model::name::ParentScope;
    fn process(
        graph: &mut Graph,
        qualified: &str,
        current_nesting: Option<NameId>,
        current: &mut ParentScope,
        untrack: &mut Vec<NameId>,
    ) {
        for part in qualified.split("::") {
            if part.is_empty() {
                *current = ParentScope::TopLevel;
                continue;
            }
            let (parent_scope, nesting_for_part) = if part.starts_with('<') {
                let attached_id = match *current {
                    ParentScope::Some(id) | ParentScope::Attached(id) => Some(id),
                    _ => current_nesting,
                };
                (attached_id.map_or(ParentScope::None, ParentScope::Attached), attached_id)
            } else {
                (*current, current_nesting)
            };
            let str_id = graph.intern_string(part.to_owned());
            let name_id = graph.add_name(str_id, parent_scope, nesting_for_part);
            untrack.push(name_id);
            *current = ParentScope::Some(name_id);
        }
    }
    let mut current_nesting = None;
    let mut current = ParentScope::None;
    let mut untrack = Vec::new();
    for entry in nesting {
        process(graph, entry, current_nesting, &mut current, &mut untrack);
        current_nesting = current.as_ref().copied();
        current = ParentScope::None;
    }
    process(graph, const_name, current_nesting, &mut current, &mut untrack);
    let found = match current {
        ParentScope::Some(name_id) | ParentScope::Attached(name_id) => {
            rubydex::resolution::Resolver::new(graph).resolve_constant(name_id)
        }
        _ => None,
    };
    for name_id in untrack {
        graph.untrack_name(name_id);
    }
    found
}

/// A superclass or mixin reference (`Rubydex::ConstantReference`).
pub enum ConstRef {
    Resolved,
    /// `UnresolvedConstantReference#name`: the reference's last segment.
    Unresolved(String),
}

/// A read view of the index: the Ruby API's Declaration / Definition accessors.
pub struct Ix<'a> {
    pub g: RwLockReadGuard<'a, Graph>,
}

impl Ix<'_> {
    fn decl(&self, id: Decl) -> Option<&Declaration> {
        self.g.declarations().get(&id)
    }

    fn namespace(&self, id: Decl) -> Option<&Namespace> {
        self.decl(id)?.as_namespace()
    }

    /// `Graph#[]`.
    pub fn lookup(&self, name: &str) -> Option<Decl> {
        let id = declaration_id_from_lookup_name(name);
        self.g.declarations().contains_key(&id).then_some(id)
    }

    /// `is_a?(Rubydex::Namespace)` (Class, Module, SingletonClass, Todo).
    pub fn is_namespace(&self, id: Decl) -> bool {
        self.namespace(id).is_some()
    }

    /// `is_a?(Rubydex::Class)`.
    pub fn is_class(&self, id: Decl) -> bool {
        matches!(self.namespace(id), Some(Namespace::Class(_)))
    }

    /// `is_a?(Rubydex::Module)`.
    pub fn is_module(&self, id: Decl) -> bool {
        matches!(self.namespace(id), Some(Namespace::Module(_)))
    }

    pub fn name(&self, id: Decl) -> &str {
        self.decl(id).map_or("", |d| d.name())
    }

    /// `Declaration#owner`.
    pub fn owner(&self, id: Decl) -> Option<Decl> {
        let owner = *self.decl(id)?.owner_id();
        self.decl(owner).map(|_| owner)
    }

    /// `Namespace#ancestors` (resolved ancestors only).
    pub fn ancestors(&self, id: Decl) -> Vec<Decl> {
        let Some(ns) = self.namespace(id) else { return Vec::new() };
        ns.ancestors()
            .iter()
            .filter_map(|a| match a {
                Ancestor::Complete(id) => Some(*id),
                Ancestor::Partial(_) => None,
            })
            .collect()
    }

    /// `Namespace#descendants`.
    pub fn descendants(&self, id: Decl) -> Vec<Decl> {
        self.namespace(id).map(|ns| ns.descendants().iter().copied().collect()).unwrap_or_default()
    }

    /// `Namespace#members`.
    pub fn members(&self, id: Decl) -> Vec<Decl> {
        self.namespace(id).map(|ns| ns.members().values().copied().collect()).unwrap_or_default()
    }

    /// `Namespace#member(name)`: declared on the namespace itself.
    pub fn member(&self, id: Decl, member: &str) -> Option<Decl> {
        self.namespace(id)?.member(&StringId::from(member)).copied()
    }

    /// `Namespace#find_member(name)`: the namespace or an ancestor.
    pub fn find_member(&self, id: Decl, member: &str) -> Option<Decl> {
        rubydex::query::find_member_in_ancestors(&self.g, id, StringId::from(member), false).ok()
    }

    pub fn definitions(&self, id: Decl) -> Vec<Def> {
        self.decl(id).map(|d| d.definitions().to_vec()).unwrap_or_default()
    }

    pub fn definition(&self, def: Def) -> Option<&Definition> {
        self.g.definitions().get(&def)
    }

    /// `Visibility#public?` and friends (`Graph#visibility`); None for declarations without one.
    pub fn visibility(&self, id: Decl) -> Option<Visibility> {
        self.g.visibility(&id)
    }

    /// `definition.location`: the file (None unless a `file://` document) and 1-based start line.
    pub fn location(&self, def: Def) -> Option<(Option<PathBuf>, u32)> {
        let d = self.definition(def)?;
        let doc = self.g.documents().get(d.uri_id())?;
        let line = doc.line_index().line_col(d.offset().start().into()).line + 1;
        let path = doc.uri().starts_with("file://").then(|| doc.file_path()).flatten();
        Some((path, line))
    }

    /// `definition.comments.map(&:string)`.
    pub fn comments(&self, def: Def) -> Vec<String> {
        self.definition(def).map(|d| d.comments().iter().map(|c| c.string().clone()).collect()).unwrap_or_default()
    }

    /// `Definition#deprecated?`.
    pub fn deprecated(&self, def: Def) -> bool {
        self.definition(def).is_some_and(|d| d.is_deprecated())
    }

    fn const_ref(&self, id: &rubydex::model::ids::ConstantReferenceId) -> Option<ConstRef> {
        let r = self.g.constant_references().get(id)?;
        let name = self.g.names().get(r.name_id())?;
        Some(match name {
            NameRef::Resolved(_) => ConstRef::Resolved,
            NameRef::Unresolved(_) => ConstRef::Unresolved(
                self.g.strings().get(name.str()).map(|s| s.as_str().to_string()).unwrap_or_default(),
            ),
        })
    }

    /// `ClassDefinition#superclass`; None for other definitions and classes without one.
    pub fn superclass(&self, def: Def) -> Option<ConstRef> {
        match self.definition(def)? {
            Definition::Class(c) => self.const_ref(c.superclass_ref()?),
            _ => None,
        }
    }

    /// `definition.mixins` as (is_extend, reference); None when the definition has none
    /// (`respond_to?(:mixins)` is false).
    pub fn mixins(&self, def: Def) -> Option<Vec<(bool, ConstRef)>> {
        let mixins = match self.definition(def)? {
            Definition::Class(c) => c.mixins(),
            Definition::Module(m) => m.mixins(),
            Definition::SingletonClass(s) => s.mixins(),
            _ => return None,
        };
        Some(
            mixins
                .iter()
                .filter_map(|m| Some((matches!(m, Mixin::Extend(_)), self.const_ref(m.constant_reference_id())?)))
                .collect(),
        )
    }

    // ---- ProjectIndexHelp ----

    /// `indexed_singleton_of`.
    pub fn singleton_of(&self, id: Decl) -> Option<Decl> {
        let name = self.name(id);
        let last = name.rsplit("::").next().unwrap_or(name);
        self.lookup(&format!("{name}::<{last}>"))
    }

    /// `indexed_singleton_member`.
    pub fn singleton_member(&self, id: Decl, member: &str) -> Option<Decl> {
        for ancestor in self.ancestors(id) {
            if let Some(singleton) = self.singleton_of(ancestor) {
                return self.find_member(singleton, member);
            }
        }
        None
    }

    /// `inherited_index_member?`.
    pub fn inherited_member(&self, scope: Decl, member: &str) -> bool {
        let name = self.name(scope);
        self.ancestors(scope).into_iter().any(|a| self.name(a) != name && self.member(a, member).is_some())
    }

    /// `fully_resolved_index_ancestry?`.
    pub fn fully_resolved_ancestry(&self, id: Decl, ignore_extend: bool) -> bool {
        self.ancestors(id).into_iter().all(|a| {
            self.definitions(a).into_iter().all(|def| {
                !matches!(self.superclass(def), Some(ConstRef::Unresolved(_)))
                    && self.mixins(def).is_none_or(|ms| {
                        !ms.iter()
                            .any(|(extend, r)| !(ignore_extend && *extend) && matches!(r, ConstRef::Unresolved(_)))
                    })
            })
        })
    }

    /// `definitions_in_other_files`: definitions in a `file://` document other than `current`,
    /// ordered by path and line.
    pub fn in_other_files(&self, defs: &[Def], current: &str) -> Vec<(Def, PathBuf, u32)> {
        let mut out: Vec<(Def, PathBuf, u32)> = defs
            .iter()
            .filter_map(|&d| {
                let (path, line) = self.location(d)?;
                let path = path?;
                (!identical(&path, Path::new(current))).then_some((d, path, line))
            })
            .collect();
        // Ruby sorts the path Strings (bytewise), not path components.
        out.sort_by(|a, b| (a.1.as_os_str().as_encoded_bytes(), a.2).cmp(&(b.1.as_os_str().as_encoded_bytes(), b.2)));
        out
    }

    // ---- IndexedMethodArity ----

    /// `indexed_member_shape`: `(min, max, has_keywords)` agreed by the member's method
    /// definitions, None when it has none or they disagree.
    pub fn member_shape(&self, member: Decl) -> Option<(usize, Option<usize>, bool)> {
        let mut shapes: Vec<Option<(usize, Option<usize>, bool)>> = Vec::new();
        let mut any = false;
        for def in self.definitions(member) {
            let Some(Definition::Method(m)) = self.definition(def) else { continue };
            any = true;
            for sig in m.signatures().as_slice() {
                let shape = shape_for(sig);
                if shape.is_some() && !shapes.contains(&shape) {
                    shapes.push(shape);
                }
            }
        }
        if !any || shapes.len() != 1 {
            return None;
        }
        shapes[0]
    }
}

/// `shape_for`.
fn shape_for(sig: &[Parameter]) -> Option<(usize, Option<usize>, bool)> {
    let count = |f: fn(&Parameter) -> bool| sig.iter().filter(|p| f(p)).count();
    if sig.iter().any(|p| matches!(p, Parameter::Forward(_))) {
        return None;
    }
    let min = count(|p| matches!(p, Parameter::RequiredPositional(_) | Parameter::Post(_)));
    let max = (!sig.iter().any(|p| matches!(p, Parameter::RestPositional(_))))
        .then(|| min + count(|p| matches!(p, Parameter::OptionalPositional(_))));
    let keywords = sig.iter().any(|p| {
        matches!(p, Parameter::RequiredKeyword(_) | Parameter::OptionalKeyword(_) | Parameter::RestKeyword(_))
    });
    Some((min, max, keywords))
}

/// `File.identical?`: both exist and are the same file.
pub fn identical(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// `same_file?`.
pub fn same_file(a: &Path, b: &Path) -> bool {
    identical(a, b) || std::path::absolute(a).ok() == std::path::absolute(b).ok()
}

/// `arity_satisfied?`.
pub fn arity_satisfied(given: usize, min: usize, max: Option<usize>) -> bool {
    given >= min && max.is_none_or(|m| given <= m)
}

/// `expected_range`.
pub fn expected_range(min: usize, max: Option<usize>) -> String {
    match max {
        None => format!("{min}+"),
        Some(m) if m == min => min.to_string(),
        Some(m) => format!("{min}..{m}"),
    }
}

// ---- node helpers (rubocop-ast on Prism) ----

/// The constant path of a class or module node (`identifier`).
pub fn identifier<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    if let Some(c) = node.as_class_node() {
        return Some(c.constant_path());
    }
    node.as_module_node().map(|m| m.constant_path())
}

fn body_range(node: &Node) -> Option<(usize, usize)> {
    let body = if let Some(c) = node.as_class_node() { c.body() } else { node.as_module_node()?.body() }?;
    let l = body.location();
    Some((l.start_offset(), l.end_offset()))
}

/// `lexical_nesting_of(node)`: the identifiers' `const_name` of the class and module ancestors
/// whose body holds the node at `start`, outermost first.
pub fn lexical_nesting(ancestors: &[Node], start: usize) -> Vec<String> {
    ancestors
        .iter()
        .filter(|a| body_range(a).is_some_and(|(s, e)| s <= start && start < e))
        .filter_map(|a| crate::helpers::const_name::const_name(&identifier(a)?))
        .collect()
}

/// rubocop-ast `ConstNode#absolute?`: the path starts with `::`.
pub fn absolute(node: &Node) -> bool {
    let mut cur = node.as_constant_path_node();
    while let Some(p) = cur {
        match p.parent() {
            None => return true,
            Some(parent) => cur = parent.as_constant_path_node(),
        }
    }
    false
}

/// `resolve_constant_in_index(const_node)`: the first segment through the lexical nesting, each
/// later one as a member of the previous. `ancestors` are the node's, outermost first.
pub fn resolve_const_node(index: &ProjectIndex, node: &Node, ancestors: &[Node]) -> Option<Decl> {
    let name = crate::helpers::const_name::const_name(node)?;
    let nesting = if absolute(node) { Vec::new() } else { lexical_nesting(ancestors, node.location().start_offset()) };
    let mut segments = name.split("::");
    let mut decl = index.resolve_constant(segments.next()?, &nesting)?;
    let ix = index.read();
    for segment in segments {
        if !ix.is_namespace(decl) {
            return None;
        }
        decl = ix.find_member(decl, segment)?;
    }
    Some(decl)
}

/// The nearest class or module ancestor (`node.each_ancestor(:class, :module).first`) resolved in
/// the index, when it is a namespace.
pub fn enclosing_namespace(index: &ProjectIndex, ancestors: &[Node]) -> Option<Decl> {
    let i = ancestors.iter().rposition(|a| a.as_class_node().is_some() || a.as_module_node().is_some())?;
    let decl = resolve_const_node(index, &identifier(&ancestors[i])?, &ancestors[..i])?;
    index.read().is_namespace(decl).then_some(decl)
}

/// AccessorMethodName / PredicatePrefix#overrides_inherited_method?: an ancestor elsewhere in the
/// project defines the method.
pub fn overrides_inherited_method(ancestors: &[Node], def: &ruby_prism::DefNode) -> bool {
    let Some(index) = get() else { return false };
    let Some(decl) = enclosing_namespace(index, ancestors) else { return false };
    let ix = index.read();
    let scope = if def.receiver().is_some() { ix.singleton_of(decl) } else { Some(decl) };
    let member = format!("{}()", String::from_utf8_lossy(def.name().as_slice()));
    scope.is_some_and(|s| ix.inherited_member(s, &member))
}

/// The node kinds that are a parser `send` (or `csend`) the index-aware cops' `on_send` sees: a
/// call, a call assigned in a multiple assignment (`recv.m, x =`, `recv[i], x =`) and the read of
/// `recv.m op= v` / `recv[i] op= v`.
pub const SEND_KINDS: &[usize] = &[
    crate::node_kind::CALL_NODE,
    crate::node_kind::CALL_TARGET_NODE,
    crate::node_kind::CALL_OPERATOR_WRITE_NODE,
    crate::node_kind::CALL_OR_WRITE_NODE,
    crate::node_kind::CALL_AND_WRITE_NODE,
    crate::node_kind::INDEX_TARGET_NODE,
    crate::node_kind::INDEX_OPERATOR_WRITE_NODE,
    crate::node_kind::INDEX_OR_WRITE_NODE,
    crate::node_kind::INDEX_AND_WRITE_NODE,
];

/// A parser `send`: receiver, method name, arguments (no block argument) and selector.
pub struct Send<'pr> {
    pub receiver: Option<Node<'pr>>,
    pub name: String,
    pub args: Vec<Node<'pr>>,
    pub selector: Option<(usize, usize)>,
}

pub fn parser_send<'pr>(node: &Node<'pr>) -> Option<Send<'pr>> {
    let name = |id: ruby_prism::ConstantId| String::from_utf8_lossy(id.as_slice()).into_owned();
    let sel = |l: Option<ruby_prism::Location>| l.map(|l| (l.start_offset(), l.end_offset()));
    if let Some(c) = node.as_call_node() {
        return Some(Send {
            receiver: c.receiver(),
            name: name(c.name()),
            args: crate::ast::call_args(&c),
            selector: sel(c.message_loc()),
        });
    }
    if let Some(c) = node.as_call_target_node() {
        let l = c.message_loc();
        return Some(Send {
            receiver: Some(c.receiver()),
            name: name(c.name()),
            args: Vec::new(),
            selector: Some((l.start_offset(), l.end_offset())),
        });
    }
    // `recv[i], x =` is `send :[]=` and `recv[i] op= v` reads `send :[]`, the selector `[i]`.
    let index = if let Some(c) = node.as_index_target_node() {
        Some((Some(c.receiver()), "[]=", c.arguments(), c.opening_loc(), c.closing_loc()))
    } else if let Some(c) = node.as_index_operator_write_node() {
        Some((c.receiver(), "[]", c.arguments(), c.opening_loc(), c.closing_loc()))
    } else if let Some(c) = node.as_index_or_write_node() {
        Some((c.receiver(), "[]", c.arguments(), c.opening_loc(), c.closing_loc()))
    } else {
        node.as_index_and_write_node().map(|c| (c.receiver(), "[]", c.arguments(), c.opening_loc(), c.closing_loc()))
    };
    if let Some((receiver, name, args, open, close)) = index {
        return Some(Send {
            receiver,
            name: name.to_string(),
            args: args.map(|a| a.arguments().iter().collect()).unwrap_or_default(),
            selector: Some((open.start_offset(), close.end_offset())),
        });
    }
    let (receiver, read, msg) = if let Some(c) = node.as_call_operator_write_node() {
        (c.receiver(), c.read_name(), c.message_loc())
    } else if let Some(c) = node.as_call_or_write_node() {
        (c.receiver(), c.read_name(), c.message_loc())
    } else {
        let c = node.as_call_and_write_node()?;
        (c.receiver(), c.read_name(), c.message_loc())
    };
    Some(Send { receiver, name: name(read), args: Vec::new(), selector: sel(msg) })
}

/// `positional_argument_count(node, callee_has_keywords)` over a call's (or `super`'s) Prism
/// arguments (the block argument is not among them): None when forwarding or double-splatting.
pub fn positional_argument_count(args: &[Node], callee_has_keywords: bool) -> Option<usize> {
    // forwards_arguments?: splat, `...`, anonymous `*` (an anonymous `**` sits inside the hash).
    if args.iter().any(|a| a.as_splat_node().is_some() || a.as_forwarding_arguments_node().is_some()) {
        return None;
    }
    let Some(hash) = args.last().and_then(|a| a.as_keyword_hash_node()) else { return Some(args.len()) };
    // double_splat?: a `**h` (an anonymous `**` is forwarded_kwrestarg, not kwsplat).
    if hash.elements().iter().any(|e| e.as_assoc_splat_node().is_some_and(|s| s.value().is_some())) {
        return None;
    }
    Some(if callee_has_keywords { args.len() - 1 } else { args.len() })
}

/// Identifiers in the file's `sym` and `str` literals (NameTypo / UnusedPrivateMethod
/// `literal_names`) and, with `with_eq` (UnusedPrivateMethod), the identifier ending each
/// interpolated string or symbol's leading fragment (`literal_name_prefixes`).
pub struct Literals {
    pub names: HashSet<String>,
    pub prefixes: HashSet<String>,
}

pub fn literals(src: &crate::source::Source, root: &Node, with_eq: bool) -> Literals {
    use ruby_prism::Visit;
    use std::sync::LazyLock;
    // Ruby's `\w` is ASCII.
    static IDENT: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]*[?!]?").unwrap());
    static IDENT_EQ: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]*[?!=]?").unwrap());
    static PREFIX: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]*\z").unwrap());
    struct V<'a> {
        re: &'a regex::Regex,
        out: Literals,
        regex_depth: usize,
        path: String,
        prefixes: bool,
    }
    impl V<'_> {
        fn scan(&mut self, bytes: &[u8]) {
            let s = String::from_utf8_lossy(bytes);
            for m in self.re.find_iter(&s) {
                self.out.names.insert(m.as_str().to_string());
            }
        }
        /// `interpolation_prefix`.
        fn prefix(&mut self, parts: &ruby_prism::NodeList) {
            if !self.prefixes || parts.iter().all(|p| p.as_string_node().is_some()) {
                return;
            }
            let Some(first) = parts.iter().next().and_then(|p| p.as_string_node()) else { return };
            let value = String::from_utf8_lossy(first.unescaped());
            // Parser splits a literal's text after each newline: a leading fragment holding one
            // ends with it, and no identifier ends there.
            if value.contains('\n') {
                return;
            }
            if let Some(m) = PREFIX.find(&value) {
                self.out.prefixes.insert(m.as_str().to_string());
            }
        }
    }
    impl<'pr> Visit<'pr> for V<'_> {
        fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
            // A regexp's `str` parts hold its source text.
            if self.regex_depth > 0 {
                self.scan(node.content_loc().as_slice());
            } else {
                self.scan(node.unescaped());
            }
        }
        fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode<'pr>) {
            self.scan(node.unescaped());
        }
        fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode<'pr>) {
            self.scan(node.content_loc().as_slice());
        }
        fn visit_match_last_line_node(&mut self, node: &ruby_prism::MatchLastLineNode<'pr>) {
            self.scan(node.content_loc().as_slice());
        }
        fn visit_interpolated_regular_expression_node(
            &mut self,
            node: &ruby_prism::InterpolatedRegularExpressionNode<'pr>,
        ) {
            self.regex_depth += 1;
            ruby_prism::visit_interpolated_regular_expression_node(self, node);
            self.regex_depth -= 1;
        }
        fn visit_interpolated_match_last_line_node(&mut self, node: &ruby_prism::InterpolatedMatchLastLineNode<'pr>) {
            self.regex_depth += 1;
            ruby_prism::visit_interpolated_match_last_line_node(self, node);
            self.regex_depth -= 1;
        }
        // Parser's `__FILE__` is a `str` of the file's path.
        fn visit_source_file_node(&mut self, _: &ruby_prism::SourceFileNode<'pr>) {
            let path = std::mem::take(&mut self.path);
            self.scan(path.as_bytes());
            self.path = path;
        }
        fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode<'pr>) {
            self.out.names.insert(String::from_utf8_lossy(node.unescaped()).into_owned());
        }
        fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
            self.prefix(&node.parts());
            ruby_prism::visit_interpolated_string_node(self, node);
        }
        fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode<'pr>) {
            self.prefix(&node.parts());
            ruby_prism::visit_interpolated_symbol_node(self, node);
        }
    }
    let mut v = V {
        re: if with_eq { &IDENT_EQ } else { &IDENT },
        out: Literals { names: HashSet::new(), prefixes: HashSet::new() },
        regex_depth: 0,
        path: src.absolute_path(),
        prefixes: with_eq,
    };
    v.visit(root);
    v.out
}
