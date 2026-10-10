//! `.rubocop.yml` loading and resolution. Section numbers refer to docs/rubocop-config-semantics.md.

pub mod glob;
pub mod target_ruby;
pub mod value;

use crate::cop::Severity;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use value::{Map, Value, parse_yaml, ruby_regex};

/// RuboCop's own default.yml, vendored from the release rbcop targets (MIT, see config/LICENSE).
pub const DEFAULT_YML: &str = include_str!("../../config/default.yml");
pub const RUBOCOP_VERSION: &str = "1.91.0";
const DOTFILE: &str = ".rubocop.yml";

/// Command-line switches that change how configs resolve.
#[derive(Clone, Default)]
pub struct Options {
    /// `-c FILE`
    pub config_file: Option<PathBuf>,
    pub force_default_config: bool,
    /// `--only` cops are enabled regardless of config.
    pub only: Vec<String>,
    pub enable_pending_cops: bool,
    pub disable_pending_cops: bool,
    pub ignore_parent_exclusion: bool,
}

/// One include/exclude entry (FilePatterns / PathUtil.match_path?).
#[derive(Clone, Debug)]
pub enum Pattern {
    Exact(String),
    Glob(String),
    Regex(Regex),
}

impl Pattern {
    fn from(v: &Value) -> Option<Pattern> {
        match v {
            Value::Regex(r) => ruby_regex(r).map(Pattern::Regex),
            v => {
                let s = v.as_string()?;
                Some(if glob::is_glob(&s) { Pattern::Glob(s) } else { Pattern::Exact(s) })
            }
        }
    }

    /// PathUtil.match_path?
    pub fn matches(&self, path: &str) -> bool {
        match self {
            Pattern::Exact(s) => s == path,
            Pattern::Regex(r) => r.is_match(path),
            Pattern::Glob(p) => {
                let p = if p.starts_with("..") {
                    expand_path(Path::new(p), &cwd()).display().to_string()
                } else {
                    p.clone()
                };
                glob::fnmatch(&p, path, false) || hidden_file_in_not_hidden_dir(&p, path)
            }
        }
    }

    fn is_absolute_or_parent(&self) -> bool {
        match self {
            Pattern::Exact(s) | Pattern::Glob(s) => s.starts_with('/') || s.starts_with(".."),
            Pattern::Regex(_) => false,
        }
    }
}

fn hidden_file_in_not_hidden_dir(pattern: &str, path: &str) -> bool {
    let p = Path::new(path);
    let hidden_file = p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) && path.contains("/.");
    let hidden_dir = p.parent().is_some_and(|d| {
        d.components().any(|c| matches!(c, Component::Normal(n) if n.to_string_lossy().starts_with('.')))
    });
    hidden_file && glob::fnmatch(pattern, path, true) && !hidden_dir
}

fn patterns(map: &Map, key: &str) -> Option<Vec<Pattern>> {
    match map.get(key)? {
        Value::Seq(items) => Some(items.iter().filter_map(Pattern::from).collect()),
        Value::Null => None,
        v => Some(Pattern::from(v).into_iter().collect()),
    }
}

/// A cop's effective settings in one resolved config.
#[derive(Clone, Debug)]
pub struct CopConfig {
    pub enabled: bool,
    pub severity: Option<Severity>,
    include: Option<Vec<Pattern>>,
    exclude: Option<Vec<Pattern>>,
    /// Department params merged under the cop's (Config#for_badge).
    pub params: Map,
}

impl CopConfig {
    pub fn str(&self, key: &str) -> Option<&str> {
        self.params.str(key)
    }
    pub fn bool(&self, key: &str) -> Option<bool> {
        self.params.bool(key)
    }
    pub fn int(&self, key: &str) -> Option<i64> {
        self.params.int(key)
    }
    pub fn strings(&self, key: &str) -> Vec<String> {
        self.params.strings(key)
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.params.get(key)
    }
    /// `cop_config[key]` as a list of Regex (AllowedPatterns style: strings become regexps).
    pub fn regexes(&self, key: &str) -> Vec<Regex> {
        match self.params.get(key) {
            Some(Value::Seq(items)) => items
                .iter()
                .filter_map(|v| match v {
                    Value::Regex(r) => ruby_regex(r),
                    v => v.as_string().and_then(|s| ruby_regex(&s)),
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// A fully merged config (one per `.rubocop.yml`, or the default).
pub struct Resolved {
    pub hash: Map,
    /// Base for relative Include patterns (base_dir_for_path_parameters of the loaded file).
    pub base_dir: PathBuf,
    /// TargetRuby#version (e.g. 3.4).
    pub target_ruby: f64,
    cops: HashMap<String, CopConfig>,
}

impl Resolved {
    fn new(
        hash: Map,
        loaded_path: Option<&Path>,
        opts: &Options,
        cop_names: &[&'static str],
        project_root: Option<&Path>,
    ) -> Self {
        let base_dir = base_dir_for(loaded_path);
        let all_cops = hash.map("AllCops").cloned().unwrap_or_default();
        let target_ruby = target_ruby::detect(&all_cops, &base_dir, loaded_path.is_some(), project_root);
        let mut r = Resolved { hash, base_dir, target_ruby, cops: HashMap::new() };
        for &name in cop_names {
            let cfg = r.build_cop(name, opts);
            r.cops.insert(name.to_string(), cfg);
        }
        r
    }

    pub fn all_cops(&self) -> Map {
        self.hash.map("AllCops").cloned().unwrap_or_default()
    }

    pub fn cop(&self, name: &str) -> &CopConfig {
        &self.cops[name]
    }

    /// Any cop's raw parameters (department merged under cop), implemented by rbcop or not.
    pub fn for_cop(&self, name: &str) -> Map {
        let mut params = name.rsplit_once('/').and_then(|(d, _)| self.hash.map(d)).cloned().unwrap_or_default();
        for (k, v) in self.hash.map(name).cloned().unwrap_or_default().0 {
            params.insert(k, v);
        }
        params
    }

    /// Config#enable_cop? + Registry#enabled?, then Config#for_badge (§4).
    fn build_cop(&self, name: &str, opts: &Options) -> CopConfig {
        let cop = self.hash.map(name).cloned().unwrap_or_default();
        let department = name.rsplit_once('/').map(|(d, _)| d);
        let dept_map = department.and_then(|d| self.hash.map(d)).cloned().unwrap_or_default();
        let dbd = self.all_cops().truthy("DisabledByDefault");
        let enabled_value = cop.get("Enabled").cloned().unwrap_or(Value::Bool(!dbd));
        let effective =
            if enabled_value == Value::Bool(true) || enabled_value == Value::Str("override_department".into()) {
                Value::Bool(true)
            } else if dept_map.get("Enabled") == Some(&Value::Bool(false)) {
                Value::Bool(false)
            } else {
                enabled_value
            };
        let new_cops = self.all_cops().str("NewCops") == Some("enable");
        let enabled = opts.only.iter().any(|o| o == name)
            || effective == Value::Bool(true)
            || (!opts.disable_pending_cops
                && effective == Value::Str("pending".into())
                && (opts.enable_pending_cops || new_cops));
        let mut params = dept_map;
        for (k, v) in cop.0 {
            params.insert(k, v);
        }
        let severity = params.str("Severity").and_then(Severity::parse);
        CopConfig {
            enabled,
            severity,
            include: patterns(&params, "Include"),
            exclude: patterns(&params, "Exclude"),
            params,
        }
    }

    /// Path relative to the config's base dir (PathUtil.relative_path).
    pub fn relative(&self, file: &Path) -> String {
        relative_path(file, &self.base_dir)
    }

    /// Cop::Base#relevant_file? for an absolute file path (§7).
    pub fn relevant(&self, cop: &CopConfig, file: &Path) -> bool {
        if cop.include.is_none() && cop.exclude.is_none() {
            return true;
        }
        let rel = self.relative(file);
        let abs = file.display().to_string();
        let included = match &cop.include {
            None => true,
            Some(pats) => {
                pats.iter().any(|p| p.matches(&rel))
                    || (!rel.starts_with("..") && pats.iter().any(|p| p.is_absolute_or_parent() && p.matches(&abs)))
                    || (rel.starts_with("..") && pats.iter().any(|p| p.matches(&abs)))
            }
        };
        let excluded = cop.exclude.as_ref().is_some_and(|pats| pats.iter().any(|p| p.matches(&rel) || p.matches(&abs)));
        included && !excluded
    }

    /// Config#file_to_exclude? (AllCops/Exclude against the absolute path).
    pub fn file_to_exclude(&self, file: &Path) -> bool {
        let abs = file.display().to_string();
        patterns(&self.all_cops(), "Exclude").unwrap_or_default().iter().any(|p| p.matches(&abs))
    }

    /// Config#file_to_include? (AllCops/Include).
    pub fn file_to_include(&self, file: &Path) -> bool {
        let rel = self.relative(file);
        let abs = file.display().to_string();
        let include = patterns(&self.all_cops(), "Include").unwrap_or_default();
        let hidden = rel.starts_with('.') && !rel.starts_with("..");
        let possibly_hidden = include.iter().any(|p| match p {
            Pattern::Regex(_) => true,
            Pattern::Exact(s) | Pattern::Glob(s) => s.starts_with('.') || s.contains("/."),
        });
        if hidden && !possibly_hidden {
            return false;
        }
        include.iter().any(|p| {
            let use_abs = p.is_absolute_or_parent() || rel.starts_with("..");
            p.matches(if use_abs { &abs } else { &rel })
        })
    }

    /// TargetFinder ruby_extensions / ruby_filenames, from AllCops/Include.
    pub fn ruby_extensions_and_names(&self) -> (Vec<String>, Vec<String>) {
        let mut exts = Vec::new();
        let mut names = Vec::new();
        for s in self.all_cops().strings("Include") {
            if let Some(ext) = s.strip_prefix("**/*.") {
                exts.push(format!(".{ext}"));
            } else {
                names.push(s.replace("**/", ""));
            }
        }
        (exts, names)
    }

    pub fn ruby_interpreters(&self) -> Vec<String> {
        self.all_cops().strings("RubyInterpreters")
    }
}

/// Loads config files and hands out the resolved config for each directory (ConfigStore + ConfigLoader).
pub struct Store {
    opts: Options,
    cwd: PathBuf,
    project_root: Option<PathBuf>,
    cop_names: Vec<&'static str>,
    default: Map,
    known_cops: Vec<String>,
    files: HashMap<PathBuf, Map>,
    gem_dirs: HashMap<String, PathBuf>,
    loaded_plugins: Vec<String>,
    dir_to_file: HashMap<PathBuf, Option<PathBuf>>,
    resolved: HashMap<Option<PathBuf>, Arc<Resolved>>,
    forced: Option<Arc<Resolved>>,
    pub warnings: Vec<String>,
}

impl Store {
    /// `cop_names` are the cops rbcop implements; their configs are precomputed per resolved config.
    pub fn new(opts: Options, cop_names: Vec<&'static str>) -> Result<Store, String> {
        let cwd = cwd();
        let default_text = DEFAULT_YML;
        let Value::Map(mut default) = parse_yaml(default_text)? else { return Err("bad default.yml".into()) };
        make_excludes_absolute(&mut default, &cwd);
        let known_cops = default.keys().filter(|k| k.contains('/')).map(String::from).collect();
        let project_root = find_last_upwards(&["Gemfile"], &cwd).or_else(|| find_last_upwards(&["gems.rb"], &cwd));
        let mut store = Store {
            opts,
            cwd,
            project_root: project_root.map(|p| p.parent().unwrap().to_path_buf()),
            cop_names,
            default,
            known_cops,
            files: HashMap::new(),
            gem_dirs: HashMap::new(),
            loaded_plugins: Vec::new(),
            dir_to_file: HashMap::new(),
            resolved: HashMap::new(),
            forced: None,
            warnings: Vec::new(),
        };
        if store.opts.force_default_config {
            let r = Resolved::new(
                store.default.clone(),
                None,
                &store.opts,
                &store.cop_names,
                store.project_root.as_deref(),
            );
            store.forced = Some(Arc::new(r));
        } else if let Some(path) = store.opts.config_file.clone() {
            let path = expand_path(&path, &store.cwd);
            let user = store.load_file(&path)?;
            let merged = store.merge_with_default(user);
            store.forced = Some(Arc::new(Resolved::new(
                merged,
                Some(&path),
                &store.opts,
                &store.cop_names,
                store.project_root.as_deref(),
            )));
        }
        Ok(store)
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// ConfigStore#for_dir.
    pub fn for_dir(&mut self, dir: &Path) -> Result<Arc<Resolved>, String> {
        if let Some(forced) = &self.forced {
            return Ok(forced.clone());
        }
        let file = match self.dir_to_file.get(dir) {
            Some(f) => f.clone(),
            None => {
                let f = self.find_config_path(dir);
                self.dir_to_file.insert(dir.to_path_buf(), f.clone());
                f
            }
        };
        if let Some(r) = self.resolved.get(&file) {
            return Ok(r.clone());
        }
        let hash = match &file {
            None => self.default.clone(),
            Some(path) => {
                let mut user = self.load_file(path)?;
                if !self.opts.ignore_parent_exclusion {
                    self.add_excludes_from_files(&mut user, path)?;
                }
                self.merge_with_default(user)
            }
        };
        let r =
            Arc::new(Resolved::new(hash, file.as_deref(), &self.opts, &self.cop_names, self.project_root.as_deref()));
        self.resolved.insert(file, r.clone());
        Ok(r)
    }

    pub fn for_file(&mut self, file: &Path) -> Result<Arc<Resolved>, String> {
        self.for_dir(file.parent().unwrap_or(Path::new("/")))
    }

    /// ConfigFinder#find_config_path (§1). None means default.yml.
    fn find_config_path(&self, dir: &Path) -> Option<PathBuf> {
        for d in dir.ancestors() {
            let candidate = d.join(DOTFILE);
            if candidate.is_file() {
                return Some(candidate);
            }
            if Some(d) == self.project_root.as_deref() {
                break;
            }
        }
        let root = self.project_root.clone();
        let mut fallbacks = Vec::new();
        if let Some(root) = root {
            fallbacks.push(root.join(".config/.rubocop.yml"));
            fallbacks.push(root.join(".config/rubocop/config.yml"));
        }
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            fallbacks.push(home.join(DOTFILE));
            let xdg = std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
            fallbacks.push(xdg.join("rubocop/config.yml"));
        }
        fallbacks.into_iter().find(|p| p.is_file())
    }

    /// ConfigLoader.load_file (§2).
    fn load_file(&mut self, path: &Path) -> Result<Map, String> {
        if let Some(m) = self.files.get(path) {
            return Ok(m.clone());
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if text.contains("<%") {
            self.warnings.push(format!("{}: ERB in config is not supported; parsed as plain YAML", path.display()));
        }
        let mut hash = match parse_yaml(&text).map_err(|e| format!("{}: {e}", path.display()))? {
            Value::Map(m) => m,
            Value::Null => Map::default(),
            _ => return Err(format!("Malformed configuration in {}", path.display())),
        };
        // Plugins (and plugin-capable `require:` gems) only matter here for their default configs.
        let mut plugins = Vec::new();
        for key in ["plugins", "require"] {
            for entry in seq(hash.remove(key)) {
                match entry {
                    Value::Str(s) => plugins.push(s),
                    Value::Map(m) => plugins.extend(m.keys().map(String::from)),
                    _ => {}
                }
            }
        }
        self.integrate_plugins(plugins)?;
        if let Some(Value::Map(gems)) = hash.remove("inherit_gem") {
            let mut inherit = seq(hash.remove("inherit_from"));
            for (gem, paths) in gems.0 {
                let dir = self.gem_dir(&gem)?;
                for p in seq(Some(paths)).into_iter().rev() {
                    if let Some(p) = p.as_string() {
                        inherit.insert(0, Value::Str(dir.join(p).display().to_string()));
                    }
                }
            }
            hash.insert("inherit_from", Value::Seq(inherit));
        }
        self.resolve_inheritance(path, &mut hash)?;
        hash.remove("inherit_from");
        self.add_missing_namespaces(&mut hash);
        make_excludes_absolute(&mut hash, &base_dir_for(Some(path)));
        self.files.insert(path.to_path_buf(), hash.clone());
        Ok(hash)
    }

    fn resolve_inheritance(&mut self, path: &Path, hash: &mut Map) -> Result<(), String> {
        let dir = path.parent().unwrap_or(Path::new("/"));
        let mut bases = Vec::new();
        for entry in seq(hash.get("inherit_from").cloned()) {
            let Some(entry) = entry.as_string() else { continue };
            if entry.starts_with("http://") || entry.starts_with("https://") {
                self.warnings.push(format!("{}: remote inherit_from {entry} is not supported", path.display()));
                continue;
            }
            let base_path = expand_path(Path::new(&entry), dir);
            if base_path == path {
                continue;
            }
            let base = self.load_file(&base_path)?;
            bases.push((base_path, base));
        }
        for (base_path, base) in bases.into_iter().rev() {
            override_department_setting_for_cops(&base, hash);
            override_enabled_for_disabled_departments(&base, hash);
            for (k, v) in base.0 {
                let Value::Map(v) = v else { continue };
                let only_base_has_include = v.contains("Include") && !hash.map(&k).is_some_and(|m| m.truthy("Include"));
                let merged = match hash.map(&k) {
                    Some(derived) => {
                        let mode = determine_inherit_mode(hash, &k);
                        merge(&v, derived, &mode, false)
                    }
                    None => v,
                };
                hash.insert(k.clone(), Value::Map(merged));
                if only_base_has_include
                    && base_path.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".rubocop"))
                {
                    let base_dir = base_path.parent().unwrap().to_path_buf();
                    if let Some(m) = hash.map_mut(&k) {
                        let rebased: Vec<Value> = seq(m.get("Include").cloned())
                            .into_iter()
                            .map(|inc| match inc.as_string() {
                                Some(s) if !matches!(inc, Value::Regex(_)) => {
                                    Value::Str(relative_path(&normalize(&base_dir.join(s)), dir))
                                }
                                _ => inc,
                            })
                            .collect();
                        m.insert("Include", Value::Seq(rebased));
                    }
                }
            }
        }
        Ok(())
    }

    /// Qualify bare cop names (`Debugger:` to `Lint/Debugger:`) when exactly one known cop matches.
    fn add_missing_namespaces(&self, hash: &mut Map) {
        for (key, _) in hash.0.iter_mut() {
            if key.contains('/') || key == "AllCops" {
                continue;
            }
            let suffix = format!("/{key}");
            let mut matches = self.known_cops.iter().filter(|c| c.ends_with(&suffix));
            if let (Some(only), None) = (matches.next(), matches.next()) {
                *key = only.clone();
            }
        }
    }

    /// ConfigLoader.add_excludes_from_files (§1).
    fn add_excludes_from_files(&mut self, config: &mut Map, config_file: &Path) -> Result<(), String> {
        let stop = self.project_root.clone();
        let mut topmost = None;
        for d in config_file.ancestors() {
            let candidate = d.join(DOTFILE);
            if candidate.is_file() {
                topmost = Some(candidate);
            }
            if Some(d) == stop.as_deref() {
                break;
            }
        }
        let Some(top) = topmost else { return Ok(()) };
        if top == config_file {
            return Ok(());
        }
        let highest = self.load_file(&top)?;
        let Some(high_excludes) = highest.map("AllCops").and_then(|a| a.get("Exclude")).cloned() else { return Ok(()) };
        let Some(all) = config.map_mut("AllCops") else { return Ok(()) };
        let mut excludes = seq(all.get("Exclude").cloned());
        let top_dir = top.parent().unwrap();
        for e in seq(Some(high_excludes)) {
            let e = match &e {
                Value::Str(s) if !s.starts_with('/') => Value::Str(top_dir.join(s).display().to_string()),
                _ => e,
            };
            if !excludes.contains(&e) {
                excludes.push(e);
            }
        }
        all.insert("Exclude", Value::Seq(excludes));
        Ok(())
    }

    /// Plugin::ConfigurationIntegrator: fold each plugin gem's config/default.yml into the defaults.
    // ponytail: assumes the LintRoller rules path is <gem>/config/default.yml, which every
    // rubocop-* plugin checked so far uses; local `require: ./file` entries (custom cops) are skipped.
    fn integrate_plugins(&mut self, names: Vec<String>) -> Result<(), String> {
        let names: Vec<String> = names
            .into_iter()
            .filter(|n| !n.starts_with('.') && !n.starts_with('/') && !self.loaded_plugins.contains(n))
            .collect();
        if names.is_empty() {
            return Ok(());
        }
        let mut combined = self.default.clone();
        let mut configured: Vec<String> = Vec::new();
        for name in names {
            self.loaded_plugins.push(name.clone());
            let dir = match self.gem_dir(&name) {
                Ok(d) => d,
                Err(e) => {
                    self.warnings.push(e);
                    continue;
                }
            };
            let path = dir.join("config/default.yml");
            if !path.is_file() {
                continue;
            }
            let mut plugin = self.load_file(&path)?;
            if let Some(Value::Map(new_all)) = plugin.get("AllCops").cloned() {
                let mut all = combined.map("AllCops").cloned().unwrap_or_default();
                for (key, value) in new_all.0 {
                    match (all.get(&key), &value) {
                        (Some(Value::Seq(existing)), Value::Seq(added)) => {
                            let mut union = existing.clone();
                            union.extend(added.iter().filter(|v| !existing.contains(v)).cloned());
                            all.insert(key.clone(), Value::Seq(union));
                            if !configured.contains(&key) {
                                configured.push(key);
                            }
                        }
                        _ if !configured.contains(&key) => {
                            all.insert(key.clone(), value);
                            configured.push(key);
                        }
                        _ => {}
                    }
                }
                plugin.insert("AllCops", Value::Map(all));
            }
            combined = merge_with_default(&combined, plugin, false);
        }
        if let (Some(all), Some(Value::Map(new_all))) =
            (self.default.map_mut("AllCops"), combined.get("AllCops").cloned())
        {
            for (k, v) in new_all.0 {
                all.insert(k, v);
            }
        }
        for (k, v) in combined.0 {
            if k == "AllCops" {
                continue;
            }
            let merged = match (self.default.get(&k), &v) {
                (Some(Value::Map(d)), Value::Map(p)) => Value::Map(merge(d, p, &Map::default(), false)),
                _ => v,
            };
            self.default.insert(k, merged);
        }
        Ok(())
    }

    /// ConfigLoaderResolver#merge_with_default (§4) against the current defaults.
    fn merge_with_default(&self, config: Map) -> Map {
        merge_with_default(&self.default, config, true)
    }

    /// Gem dir for inherit_gem and plugins, as `Gem::Specification.find_by_name(name).gem_dir` reports it.
    fn gem_dir(&mut self, gem: &str) -> Result<PathBuf, String> {
        if let Some(d) = self.gem_dirs.get(gem) {
            return Ok(d.clone());
        }
        // Gem names come from config files; anything else must never reach the ruby command line.
        if gem.is_empty()
            || gem.starts_with('-')
            || !gem.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
        {
            return Err(format!("invalid gem name {gem:?} in config"));
        }
        let out = std::process::Command::new("ruby")
            .args(["-e", "print Gem::Specification.find_by_name(ARGV[0]).gem_dir", "--", gem])
            .current_dir(&self.cwd)
            .output()
            .map_err(|e| format!("inherit_gem {gem}: cannot run ruby: {e}"))?;
        if !out.status.success() {
            return Err(format!("inherit_gem {gem}: gem not found ({})", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).into_owned());
        self.gem_dirs.insert(gem.to_string(), dir.clone());
        Ok(dir)
    }
}

/// ConfigLoaderResolver#merge_with_default (§4).
fn merge_with_default(base_default: &Map, mut config: Map, unset_nil: bool) -> Map {
    let all = config.map("AllCops").cloned().unwrap_or_default();
    let dbd = all.truthy("DisabledByDefault");
    let ebd = all.truthy("EnabledByDefault");
    let mut default = base_default.clone();
    if dbd || ebd {
        for (_, v) in default.0.iter_mut() {
            if let Value::Map(m) = v {
                m.insert("Enabled", Value::Bool(!dbd));
            }
        }
    }
    if dbd {
        // handle_disabled_by_default: a department enabled by the user gets its cops' real defaults back.
        let departments: Vec<String> = config
            .0
            .iter()
            .filter(|(k, v)| !k.contains('/') && v.as_map().is_some_and(|m| m.truthy("Enabled")))
            .map(|(k, _)| k.clone())
            .collect();
        for dept in departments {
            let prefix = format!("{dept}/");
            for (cop, params) in default.0.iter_mut() {
                if cop.starts_with(&prefix)
                    && let (Value::Map(p), Some(orig)) = (params, base_default.map(cop).and_then(|m| m.get("Enabled")))
                {
                    p.insert("Enabled", orig.clone());
                }
            }
        }
        for (_, v) in config.0.iter_mut() {
            if let Value::Map(m) = v
                && !m.contains("Enabled")
            {
                m.0.insert(0, ("Enabled".into(), Value::Bool(true)));
            }
        }
    }
    override_enabled_for_disabled_departments(&default, &mut config);
    let mode = config.map("inherit_mode").cloned().unwrap_or_default();
    merge(&default, &config, &mode, unset_nil)
}

/// `Array(value)` for YAML values.
fn seq(v: Option<Value>) -> Vec<Value> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Seq(items)) => items,
        Some(v) => vec![v],
    }
}

fn disabled(hash: &Map, dept: &str) -> bool {
    hash.map(dept).is_some_and(|m| m.get("Enabled") == Some(&Value::Bool(false)))
}

fn override_department_setting_for_cops(base: &Map, derived: &mut Map) {
    let keys: Vec<String> = derived.keys().map(String::from).collect();
    for key in keys {
        let Some((dept, _)) = key.rsplit_once('/') else { continue };
        if !(disabled(derived, dept) || disabled(base, dept)) {
            continue;
        }
        if let Some(m) = derived.map_mut(&key)
            && m.truthy("Enabled")
        {
            m.insert("Enabled", Value::Str("override_department".into()));
        }
    }
}

fn override_enabled_for_disabled_departments(base: &Map, derived: &mut Map) {
    let mut to_disable = Vec::new();
    for key in derived.keys() {
        if disabled(derived, key) {
            let prefix = format!("{key}/");
            to_disable.extend(base.keys().filter(|k| k.starts_with(&prefix)).map(String::from));
        }
    }
    for cop in to_disable {
        if base.map(&cop).and_then(|m| m.get("Enabled")) != Some(&Value::Bool(true)) {
            continue;
        }
        match derived.map_mut(&cop) {
            Some(m) => {
                if !m.contains("Enabled") {
                    m.0.insert(0, ("Enabled".into(), Value::Bool(false)));
                }
            }
            None => {
                let mut m = Map::default();
                m.insert("Enabled", Value::Bool(false));
                derived.0.insert(0, (cop, Value::Map(m)));
            }
        }
    }
}

fn determine_inherit_mode(hash: &Map, key: &str) -> Map {
    hash.map(key).and_then(|m| m.map("inherit_mode")).or_else(|| hash.map("inherit_mode")).cloned().unwrap_or_default()
}

fn mode_lists(mode: Option<&Map>, which: &str, key: &str) -> bool {
    mode.is_some_and(|m| m.strings(which).iter().any(|k| k == key))
}

fn should_union(derived: &Map, base: &Map, root_mode: &Map, key: &str) -> bool {
    if !matches!(base.get(key), Some(Value::Seq(_))) && !matches!(derived.get(key), Some(Value::Seq(_))) {
        return false;
    }
    for mode in [derived.map("inherit_mode"), base.map("inherit_mode")] {
        if mode_lists(mode, "override", key) {
            return false;
        }
        if mode_lists(mode, "merge", key) {
            return true;
        }
    }
    mode_lists(Some(root_mode), "merge", key)
}

/// ConfigLoaderResolver#merge (§3).
fn merge(base: &Map, derived: &Map, root_mode: &Map, unset_nil: bool) -> Map {
    let mut result = base.clone();
    for (key, dv) in &derived.0 {
        let Some(bv) = base.get(key) else {
            result.insert(key.clone(), dv.clone());
            continue;
        };
        if unset_nil && *dv == Value::Null {
            result.remove(key);
        } else if let (Value::Map(b), Value::Map(d)) = (bv, dv) {
            result.insert(key.clone(), Value::Map(merge(b, d, root_mode, unset_nil)));
        } else if should_union(derived, base, root_mode, key) {
            let mut union = seq(Some(bv.clone()));
            for v in seq(Some(dv.clone())) {
                if !union.contains(&v) {
                    union.push(v);
                }
            }
            result.insert(key.clone(), Value::Seq(union));
        } else {
            result.insert(key.clone(), dv.clone());
        }
    }
    result
}

/// Config#make_excludes_absolute: every `Exclude` string becomes absolute against `base`.
fn make_excludes_absolute(hash: &mut Map, base: &Path) {
    for (_, v) in hash.0.iter_mut() {
        let Value::Map(m) = v else { continue };
        let Some(Value::Seq(items)) = m.get_mut("Exclude") else { continue };
        for item in items.iter_mut() {
            if let Value::Str(s) = item
                && !s.starts_with('/')
            {
                *s = normalize(&base.join(&*s)).display().to_string();
            }
        }
    }
}

/// Config#base_dir_for_path_parameters (§5).
fn base_dir_for(loaded: Option<&Path>) -> PathBuf {
    let home_dotfile = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(DOTFILE));
    match loaded {
        Some(p)
            if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".rubocop"))
                && Some(p) != home_dotfile.as_deref() =>
        {
            p.parent().unwrap().to_path_buf()
        }
        _ => cwd(),
    }
}

/// Dir.pwd (no symlink resolution, like Ruby).
pub fn cwd() -> PathBuf {
    std::env::var_os("PWD")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && same_dir(p, &std::env::current_dir().unwrap_or_default()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a.canonicalize().ok() == b.canonicalize().ok()
}

/// File.expand_path(path, base): absolute, with `.` and `..` folded, symlinks untouched.
pub fn expand_path(path: &Path, base: &Path) -> PathBuf {
    normalize(&if path.is_absolute() { path.to_path_buf() } else { base.join(path) })
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// PathUtil.relative_path(path, base).
pub fn relative_path(path: &Path, base: &Path) -> String {
    if let Ok(rest) = path.strip_prefix(base) {
        return rest.display().to_string();
    }
    let (p, b): (Vec<_>, Vec<_>) = (path.components().collect(), base.components().collect());
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in &p[common..] {
        out.push(c);
    }
    out.display().to_string()
}

/// Topmost file named one of `names` from `start` up to `/` (FileFinder#find_last_file_upwards).
fn find_last_upwards(names: &[&str], start: &Path) -> Option<PathBuf> {
    let mut last = None;
    for d in start.ancestors() {
        for n in names {
            let candidate = d.join(n);
            if candidate.is_file() {
                last = Some(candidate);
            }
        }
    }
    last
}
