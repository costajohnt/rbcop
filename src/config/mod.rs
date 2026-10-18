//! `.rubocop.yml` loading and resolution. Section numbers refer to docs/rubocop-config-semantics.md.

pub mod glob;
pub mod target_ruby;
pub mod validator;
pub mod value;

use crate::cop::Severity;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use value::{Map, Value, parse_yaml, parse_yaml_file, ruby_regex};

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
    /// `--preview` / `--no-preview` (ConfigLoader.preview); wins over `AllCops: Preview`.
    pub preview: Option<bool>,
    /// `--ignore-unrecognized-cops`: unknown cop names in config are a warning, not an error.
    pub ignore_unrecognized_cops: bool,
    /// Rainbow.enabled while configs load (config validation warnings are painted).
    pub color: bool,
    /// `--safe`: only cops whose `Safe` is truthy are enabled (Registry#enabled_cop_name?).
    pub safe: bool,
    /// `--enable-all-cops` (true) / `--disable-all-cops` (false): ConfigLoader.enabled_by_default /
    /// disabled_by_default, which win over `AllCops: EnabledByDefault / DisabledByDefault`.
    pub all_cops: Option<bool>,
}

/// One include/exclude entry (FilePatterns / PathUtil.match_path?).
#[derive(Clone, Debug)]
pub enum Pattern {
    Exact(String),
    Glob(String),
    Regex(value::RubyRegex),
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
            Pattern::Exact(s) => s == path || hidden_file_in_not_hidden_dir(s, path),
            Pattern::Regex(r) => r.is_match(path),
            // `pattern == path` first: a file named `[x].rb` matches itself.
            Pattern::Glob(p) if p == path => true,
            Pattern::Glob(p) => {
                let p: std::borrow::Cow<str> = if p.starts_with("..") {
                    expand_path(Path::new(p), &cwd()).display().to_string().into()
                } else {
                    p.into()
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
    /// `cop_config[key].to_s` of a String or Symbol (`Mode: :conservative` reads as `conservative`).
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.params.get(key)? {
            Value::Sym(s) => Some(s.strip_prefix(':').unwrap_or(s)),
            _ => self.params.str(key),
        }
    }
    /// `cop_config[key]` as Ruby truthiness: a non-boolean value (`IgnoreMacros: 'true'`) is true;
    /// None when absent or null.
    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.params.get(key)? {
            Value::Null => None,
            Value::Bool(b) => Some(*b),
            _ => Some(true),
        }
    }
    /// MinBodyLength#min_body_length / MinBranchesCount#min_branches_count: `cop_config[key] ||
    /// default`, which must be a positive Integer or the cop raises (a cop error at the node it
    /// was visiting, via runner::guarded).
    pub fn positive_int(&self, key: &str, default: i64) -> usize {
        match self.params.get(key) {
            None | Some(Value::Null | Value::Bool(false)) => default as usize,
            Some(Value::Int(i)) if *i > 0 => *i as usize,
            _ => panic!("{key} needs to be a positive integer!"),
        }
    }
    pub fn int(&self, key: &str) -> Option<i64> {
        self.params.int(key)
    }
    pub fn strings(&self, key: &str) -> Vec<String> {
        self.params.strings(key)
    }
    /// A list setting a cop reads with `cop_config.fetch(key, []).include?(name)` (a missing key
    /// is empty; `includes_bare` for a bare `cop_config[key]`): on a String (`AllowedVariables: FOO_BAR`) Ruby's include? is a substring test, on a Hash a key
    /// test; nil and other scalars have no include? (a cop error at the visited node).
    pub fn includes(&self, key: &str, name: &str) -> bool {
        match self.params.get(key) {
            None => false,
            Some(Value::Str(s)) => s.contains(name),
            Some(Value::Seq(items)) => items.iter().any(|i| i.as_string().is_some_and(|s| s == name)),
            Some(Value::Map(m)) => m.contains(name),
            Some(_) => panic!("undefined method 'include?' for {key}"),
        }
    }
    /// `cop_config[key]` read bare, with no `fetch` default or `|| []`: a key a user config set to
    /// `~` is gone after the merge, so the read is nil and the cop's first call on it raises (a cop
    /// error at the visited node). Then as `includes`.
    pub fn includes_bare(&self, key: &str, name: &str) -> bool {
        self.bare(key);
        self.includes(key, name)
    }
    /// `cop_config[key]` read bare (see `includes_bare`), then as `array`.
    pub fn array_bare(&self, key: &str) -> Vec<String> {
        self.bare(key);
        self.array(key)
    }
    /// Panics on a missing key: nil has none of the list methods the cop calls next.
    pub fn bare(&self, key: &str) {
        if self.params.get(key).is_none() {
            panic!("undefined method for nil ({key})");
        }
    }
    /// `cop_config.fetch(key, default).include?(name)`: a key set to `~` reads the default.
    pub fn includes_or(&self, key: &str, name: &str, default: &[&str]) -> bool {
        match self.params.get(key) {
            None => default.contains(&name),
            Some(_) => self.includes(key, name),
        }
    }
    /// `(cop_config[key] || []).include?(name)`
    pub fn includes_or_empty(&self, key: &str, name: &str) -> bool {
        !matches!(self.params.get(key), Some(Value::Null | Value::Bool(false))) && self.includes(key, name)
    }
    /// A list setting a cop reads with `cop_config.fetch(key, [])` (`array_bare` for a bare read)
    /// and then calls an Array-only method on (`map`, `any?`, `each`, `join`, `+`): anything but an
    /// Array (a String, nil, a scalar) raises, a cop error at the visited node.
    pub fn array(&self, key: &str) -> Vec<String> {
        match self.params.get(key) {
            None => Vec::new(),
            Some(Value::Seq(items)) => items.iter().filter_map(Value::as_string).collect(),
            Some(_) => panic!("{key} is not an Array"),
        }
    }
    /// `(cop_config[key] || [])`, then an Array-only method.
    pub fn array_or_empty(&self, key: &str) -> Vec<String> {
        match self.params.get(key) {
            Some(Value::Null | Value::Bool(false)) => Vec::new(),
            _ => self.array(key),
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.params.get(key)
    }
    /// `Array(cop_config[key])` as a list of Regex (AllowedPatterns style: strings become regexps).
    pub fn regexes(&self, key: &str) -> Vec<value::RubyRegex> {
        let one = |v: &Value| match v {
            Value::Regex(r) => ruby_regex(r),
            v => v.as_string().and_then(|s| ruby_regex(&s)),
        };
        match self.params.get(key) {
            Some(Value::Seq(items)) => items.iter().filter_map(one).collect(),
            None | Some(Value::Null | Value::Map(_)) => Vec::new(),
            Some(v) => one(v).into_iter().collect(),
        }
    }
    /// A pattern list read bare and mapped (`cop_config.fetch(key, []).map { Regexp.new(_1) }`):
    /// anything but an Array raises, a cop error at the visited node.
    pub fn regexes_array(&self, key: &str) -> Vec<value::RubyRegex> {
        match self.params.get(key) {
            None | Some(Value::Seq(_)) => self.regexes(key),
            Some(_) => panic!("{key} is not an Array"),
        }
    }
}

/// FNV-1a: SipHash's DoS resistance buys nothing for cop names, and its cost showed up per file.
#[derive(Default)]
struct Fnv(u64);

impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}

/// A fully merged config (one per `.rubocop.yml`, or the default).
#[derive(Clone)]
pub struct Resolved {
    pub hash: Map,
    /// Base for relative Include patterns (base_dir_for_path_parameters of the loaded file).
    pub base_dir: PathBuf,
    /// TargetRuby#version (e.g. 3.4).
    pub target_ruby: f64,
    /// Looked up twice per cop and file, so with a cheap hash (the keys are cop names).
    cops: HashMap<String, CopConfig, std::hash::BuildHasherDefault<Fnv>>,
    /// (cop, warning) for configuration under a cop's deprecated name, which Config#for_cop warns
    /// about the first time the cop's config is asked for.
    pub deprecated_warnings: Vec<(&'static str, String)>,
    /// Cop-like keys no registered cop has (see Store::unregistered).
    unregistered: Vec<String>,
    /// The options the cops were resolved with (Registry#enabled_cop_name? for other cop names).
    opts: Options,
    /// for_cop of every cop-like key, built on first use: cops ask for other cops' settings per
    /// node, and building one takes two linear scans of the whole config and a clone.
    for_cop_cache: std::sync::OnceLock<HashMap<String, Map>>,
    /// Config#gem_versions_in_target, read on first use.
    gem_versions: std::sync::OnceLock<Option<HashMap<String, String>>>,
    /// rubocop-rspec `RSpec: Language` (RuboCop::RSpec::Language.config), read on first use.
    pub rspec_language: std::sync::OnceLock<std::sync::Arc<crate::helpers::rspec::Language>>,
    /// The native plugins loaded (by any config) when this config's first file was inspected:
    /// Runner caches each config's team from then, so plugins loaded later add no cops to it.
    pub team_plugins: std::sync::OnceLock<Vec<&'static str>>,
}

impl Resolved {
    /// Whether a cop is in this config's team (Runner#mobilized_cop_classes): RuboCop's own always,
    /// a native plugin's when the plugin was loaded before the team was mobilized (see
    /// `team_plugins`; until then, when it is loaded at all).
    pub fn team_has(&self, cop: &str) -> bool {
        match crate::helpers::cop_registry::plugin_of(cop) {
            None => true,
            Some(p) => self
                .team_plugins
                .get()
                .map_or_else(|| crate::helpers::cop_registry::plugin_loaded(p.gem), |gems| gems.contains(&p.gem)),
        }
    }

    /// Fixes this config's team at the plugins loaded now (its first file is about to be inspected).
    pub fn mobilize_team(&self) {
        self.team_plugins
            .get_or_init(|| crate::helpers::cop_registry::loaded_plugins().iter().map(|p| p.gem).collect());
    }

    fn new(hash: Map, loaded_path: Option<&Path>, opts: &Options, cop_names: &[&'static str]) -> Self {
        let base_dir = base_dir_for(loaded_path);
        let all_cops = hash.map("AllCops").cloned().unwrap_or_default();
        let target_ruby = target_ruby::detect(&all_cops, &base_dir);
        let mut r = Resolved {
            hash,
            base_dir,
            target_ruby,
            cops: HashMap::default(),
            deprecated_warnings: Vec::new(),
            unregistered: Vec::new(),
            opts: opts.clone(),
            for_cop_cache: std::sync::OnceLock::new(),
            gem_versions: std::sync::OnceLock::new(),
            rspec_language: std::sync::OnceLock::new(),
            team_plugins: std::sync::OnceLock::new(),
        };
        for &name in cop_names {
            let cfg = r.build_cop(name, opts);
            r.cops.insert(name.to_string(), cfg);
            for old in validator::deprecated_names_for(name) {
                if r.hash.map(&old).is_some_and(|m| !m.0.is_empty()) {
                    let path = loaded_path.map(|p| p.display().to_string()).unwrap_or_default();
                    let msg = format!("Warning: Using `{old}` configuration in {path} for `{name}`.\n");
                    r.deprecated_warnings.push((name, msg));
                }
            }
        }
        r
    }

    /// Config#gem_versions_in_target: the locked version of each gem in the Gemfile.lock (or
    /// gems.locked) found upwards from the base dir, or None without a lockfile.
    pub fn gem_versions_in_target(&self) -> Option<&HashMap<String, String>> {
        self.gem_versions
            .get_or_init(|| {
                let file = ["Gemfile.lock", "gems.locked"]
                    .iter()
                    .find_map(|n| self.base_dir.ancestors().map(|d| d.join(n)).find(|f| f.is_file()))?;
                let text = std::fs::read_to_string(file).unwrap_or_default();
                // Bundler::LockfileParser resolves PATH sources against Bundler.root, which needs a
                // Gemfile, gems.rb or .bundle upwards from the working directory; without one it
                // raises GemfileNotFound and RuboCop's Lockfile reports no gems.
                let has_path = text.lines().any(|l| l.trim_end_matches('\r') == "PATH");
                if has_path && std::env::var_os("BUNDLE_GEMFILE").is_none() {
                    let cwd = cwd();
                    // Bundler.default_bundle_dir ignores the user's ~/.bundle.
                    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".bundle"));
                    let root = cwd.ancestors().any(|d| {
                        d.join("gems.rb").is_file()
                            || d.join("Gemfile").is_file()
                            || (d.join(".bundle").is_dir() && home.as_deref() != Some(&d.join(".bundle")))
                    });
                    if !root {
                        return Some(HashMap::new());
                    }
                }
                Some(locked_gem_versions(&text))
            })
            .as_ref()
    }

    /// Whether the target locks `gem` at a version `>= min` (Base#target_satisfies_all_gem_version_requirements?).
    pub fn gem_at_least(&self, gem: &str, min: &str) -> bool {
        let locked = self.gem_versions_in_target().and_then(|v| v.get(gem)).and_then(|v| gem_version(v));
        locked.is_some_and(|v| Some(v) >= gem_version(min))
    }

    /// TargetRailsVersion.resolve: `AllCops: TargetRailsVersion`, else the locked `railties`
    /// major.minor, else 5.0.
    pub fn target_rails_version(&self) -> f64 {
        match self.all_cops().get("TargetRailsVersion") {
            Some(Value::Float(f)) => return *f,
            Some(Value::Int(i)) => return *i as f64,
            Some(Value::Str(s)) => return ruby_to_f(s),
            Some(Value::Null) | None => {}
            // ponytail: `true.to_f` raises in Ruby; other scalars are not valid configs.
            Some(_) => return 0.0,
        }
        let Some(v) = self.gem_versions_in_target().and_then(|v| v.get("railties")) else { return 5.0 };
        let mut parts = v.split('.');
        let major = parts.next().unwrap_or("0");
        let minor = parts.next().filter(|m| m.bytes().all(|b| b.is_ascii_digit())).unwrap_or("0");
        format!("{major}.{minor}").parse().unwrap_or(5.0)
    }

    pub fn all_cops(&self) -> &Map {
        static EMPTY: Map = Map(Vec::new());
        self.hash.map("AllCops").unwrap_or(&EMPTY)
    }

    pub fn cop(&self, name: &str) -> &CopConfig {
        &self.cops[name]
    }

    /// Any cop's raw parameters (department merged under cop), implemented by rbcop or not.
    pub fn for_cop(&self, name: &str) -> std::borrow::Cow<'_, Map> {
        let cache = self.for_cop_cache.get_or_init(|| {
            self.hash.keys().filter(|k| k.contains('/')).map(|k| (k.to_string(), self.merged_for_cop(k))).collect()
        });
        match cache.get(name) {
            Some(m) => std::borrow::Cow::Borrowed(m),
            None => std::borrow::Cow::Owned(self.merged_for_cop(name)),
        }
    }

    fn merged_for_cop(&self, name: &str) -> Map {
        let mut params = name.rsplit_once('/').and_then(|(d, _)| self.hash.map(d)).cloned().unwrap_or_default();
        for (k, v) in self.hash.map(name).cloned().unwrap_or_default().0 {
            params.insert(k, v);
        }
        params
    }

    /// Config#enable_cop?: the `Enabled` value Config#for_cop reports (true, false, `pending`, ...).
    pub fn enable_cop(&self, name: &str) -> Value {
        let cop = self.hash.map(name);
        let enabled_value = cop.and_then(|c| c.get("Enabled")).cloned();
        if enabled_value == Some(Value::Bool(true)) || name == "Lint/Syntax" {
            return Value::Bool(true);
        }
        let dbd = self.all_cops().truthy("DisabledByDefault");
        let enabled_value = enabled_value.unwrap_or(Value::Bool(!dbd));
        let dept = name.rsplit_once('/').and_then(|(d, _)| self.hash.map(d));
        if enabled_value == Value::Str("override_department".into()) {
            Value::Bool(true)
        } else if dept.is_some_and(|d| d.get("Enabled") == Some(&Value::Bool(false))) {
            Value::Bool(false)
        } else {
            enabled_value
        }
    }

    /// Config#new_cops_setting_for: the department's `NewCops`, else `AllCops: NewCops`, as `to_s`.
    fn new_cops_setting(&self, name: &str) -> String {
        let set = |v: Option<&Value>| v.filter(|v| !matches!(v, Value::Null | Value::Bool(false))).cloned();
        let dept = name.rsplit_once('/').and_then(|(d, _)| self.hash.map(d));
        set(dept.and_then(|d| d.get("NewCops")))
            .or_else(|| set(self.hash.map("AllCops").and_then(|a| a.get("NewCops"))))
            .and_then(|v| v.as_string())
            .unwrap_or_default()
    }

    /// Config#enabled_new_cop? (`covered` false) and #new_cops_covered? (`covered` true, where
    /// `disable` also settles the cop): `enable`, `disable`, or a version covering VersionAdded.
    fn new_cops(&self, name: &str, covered: bool) -> bool {
        match self.new_cops_setting(name).as_str() {
            "enable" => true,
            "disable" => covered,
            "" | "pending" => false,
            pinned => {
                let added = self.hash.map(name).and_then(|m| m.get("VersionAdded")).and_then(Value::as_string);
                match (added.as_deref().and_then(gem_version), gem_version(pinned)) {
                    (Some(added), Some(pinned)) => added <= pinned,
                    _ => false,
                }
            }
        }
    }

    /// Config#enabled_new_cop?
    pub fn enabled_new_cop(&self, name: &str) -> bool {
        self.new_cops(name, false)
    }

    /// A cop name some registered cop has: in the defaults (core and integrated plugins) or, as
    /// Store::unregistered decides, defined by a loaded plugin.
    pub fn registered(&self, name: &str) -> bool {
        !self.unregistered.iter().any(|n| n == name)
    }

    /// Config#pending_cops (only registered cops, as PendingCopsReporter keeps them): every
    /// `Enabled: pending` cop no `NewCops` setting settles, outside a disabled department, in config
    /// order, with its VersionAdded.
    // ponytail: "registered" is "a key with a department"; RuboCop rejects unknown cop names in
    // config before this runs, which rbcop does not validate.
    pub fn pending_cops(&self) -> Vec<(&str, String)> {
        let mut out = Vec::new();
        for (name, v) in &self.hash.0 {
            let Some((dept, _)) = name.rsplit_once('/') else { continue };
            // PendingCopsReporter#pending_cops_only_qualified
            if self.unregistered.contains(name) {
                continue;
            }
            if self.hash.map(dept).is_some_and(|d| d.get("Enabled") == Some(&Value::Bool(false))) {
                continue;
            }
            let Value::Map(meta) = v else { continue };
            if meta.str("Enabled") != Some("pending") || self.new_cops(name, true) {
                continue;
            }
            let version = match meta.get("VersionAdded") {
                None | Some(Value::Null | Value::Bool(false)) => "N/A".to_string(),
                Some(v) => v.as_string().unwrap_or_default(),
            };
            out.push((name.as_str(), version));
        }
        out
    }

    /// Config#preview?(options).
    pub fn preview(&self, opts: &Options) -> bool {
        opts.preview.unwrap_or_else(|| self.all_cops().get("Preview") == Some(&Value::Bool(true)))
    }

    /// Registry#enabled_pending_cop? for a cop whose Config#for_cop `Enabled` is `enabled`.
    pub fn enabled_pending_cop(&self, name: &str, enabled: &Value) -> bool {
        !self.opts.disable_pending_cops
            && *enabled == Value::Str("pending".into())
            && (self.opts.enable_pending_cops || self.new_cops(name, false))
    }

    /// Registry#enabled_cop_name?: `effective` is Config#for_cop's `Enabled`, `cop` its params.
    fn enabled_cop_name(&self, name: &str, effective: &Value, cop: &Map, opts: &Options) -> bool {
        let safe = || !matches!(cop.get("Safe"), Some(Value::Null | Value::Bool(false)));
        opts.only.iter().any(|o| o == name)
            || ((*effective == Value::Bool(true)
                || self.enabled_pending_cop(name, effective)
                || (*effective == Value::Str("preview".into()) && self.preview(opts)))
                && (!opts.safe || safe()))
    }

    /// Registry#enabled? for any cop name, ported or not (Lint/Syntax included): the one place
    /// that applies config, `--only`, `--safe`, pending/preview options and `--disable-all-cops`.
    pub fn cop_enabled(&self, name: &str) -> bool {
        match self.cops.get(name) {
            Some(c) => c.enabled,
            None => {
                static EMPTY: Map = Map(Vec::new());
                let cop = self.hash.map(name).unwrap_or(&EMPTY);
                self.enabled_cop_name(name, &self.enable_cop(name), cop, &self.opts)
            }
        }
    }

    /// Config#enable_cop? + Registry#enabled?, then Config#for_badge (§4).
    fn build_cop(&self, name: &str, opts: &Options) -> CopConfig {
        let mut cop = self.hash.map(name).cloned().unwrap_or_default();
        let department = name.rsplit_once('/').map(|(d, _)| d);
        let dept_map = department.and_then(|d| self.hash.map(d)).cloned().unwrap_or_default();
        let mut effective = self.enable_cop(name);
        // Config#for_cop: configuration under a deprecated name (a rename that only warns) is
        // merged over the cop's, Enabled included.
        for old in validator::deprecated_names_for(name) {
            for (k, v) in self.hash.map(&old).cloned().unwrap_or_default().0 {
                if k == "Enabled" {
                    effective = v.clone();
                }
                cop.insert(k, v);
            }
        }
        let enabled = self.enabled_cop_name(name, &effective, &cop, opts);
        // Config#for_badge: the department's Exclude and the cop's are unioned.
        let union = match (dept_map.get("Exclude"), cop.get("Exclude")) {
            (Some(Value::Seq(d)), Some(Value::Seq(c))) if department.is_some() => {
                let mut all = d.to_vec();
                all.extend(c.iter().filter(|v| !d.contains(v)).cloned());
                Some(Value::Seq(all.into()))
            }
            _ => None,
        };
        let mut params = dept_map;
        for (k, v) in cop.0 {
            params.insert(k, v);
        }
        if let Some(u) = union {
            params.insert("Exclude", u);
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
        patterns(self.all_cops(), "Exclude").unwrap_or_default().iter().any(|p| p.matches(&abs))
    }

    /// Config#file_to_include? (AllCops/Include).
    pub fn file_to_include(&self, file: &Path) -> bool {
        let rel = self.relative(file);
        let abs = file.display().to_string();
        let include = patterns(self.all_cops(), "Include").unwrap_or_default();
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
                // `sub`: only the first `**/` goes.
                names.push(s.replacen("**/", "", 1));
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
    /// Each loaded file with what loading it wrote to stderr: ConfigLoader.load_file is not
    /// cached, so every later load of the file repeats its validation warnings.
    files: HashMap<PathBuf, (Map, validator::Output)>,
    /// Everything config loading has written to stderr so far.
    emitted: validator::Output,
    gem_dirs: HashMap<String, PathBuf>,
    features: HashMap<String, Result<Feature, String>>,
    /// The `plugins:`/`require:` entry whose loading failed (integrate_plugins).
    failed_entry: Option<String>,
    loaded_plugins: Vec<String>,
    /// A `require:` rbcop cannot load (see validator::Validator::custom_cops).
    custom_cops: bool,
    /// Loaded plugins rbcop does not implement that define no cops (rubocop-erb, rubocop-md):
    /// their effect is Ruby code run inside RuboCop. Each with its default.yml `AllCops: Include`.
    copless_plugins: Vec<(String, Vec<Pattern>)>,
    /// Class and module names in loaded plugins' lib/ (see ruby_constants).
    plugin_constants: std::collections::HashSet<String>,
    /// What directives and `--only` cannot enumerate (cop_registry::set_open): a `require:`
    /// whose cops rbcop cannot read, and the class names of a gem (or local file) in which no cop
    /// class was found. Where cop classes were found, they are the registry.
    open_custom: bool,
    open_constants: std::collections::HashSet<String>,
    dir_to_file: HashMap<PathBuf, Option<PathBuf>>,
    /// inherit_from files being loaded (outermost first), to detect cycles.
    loading: Vec<PathBuf>,
    resolved: HashMap<Option<PathBuf>, Arc<Resolved>>,
    forced: Option<Arc<Resolved>>,
    pub warnings: Vec<String>,
}

impl Store {
    /// `cop_names` are the cops rbcop implements; their configs are precomputed per resolved config.
    pub fn new(opts: Options, cop_names: Vec<&'static str>) -> Result<Store, String> {
        let cwd = cwd();
        let default_text = DEFAULT_YML;
        let Value::Map(default) = parse_yaml(default_text)? else { return Err("bad default.yml".into()) };
        let mut default = Arc::unwrap_or_clone(default);
        make_excludes_absolute(&mut default, &cwd);
        let project_root = find_last_upwards(&["Gemfile"], &cwd).or_else(|| find_last_upwards(&["gems.rb"], &cwd));
        let mut store = Store {
            opts,
            cwd,
            project_root: project_root.map(|p| p.parent().unwrap().to_path_buf()),
            cop_names,
            default,
            files: HashMap::new(),
            emitted: Vec::new(),
            gem_dirs: HashMap::new(),
            features: HashMap::new(),
            failed_entry: None,
            loaded_plugins: Vec::new(),
            custom_cops: false,
            copless_plugins: Vec::new(),
            plugin_constants: Default::default(),
            open_custom: false,
            open_constants: Default::default(),
            dir_to_file: HashMap::new(),
            loading: Vec::new(),
            resolved: HashMap::new(),
            forced: None,
            warnings: Vec::new(),
        };
        if store.opts.force_default_config {
            let r = Resolved::new(
                apply_default_overrides(&store.default, &store.opts),
                None,
                &store.opts,
                &store.cop_names,
            );
            store.forced = Some(Arc::new(r));
        } else if let Some(path) = store.opts.config_file.clone() {
            let path = expand_path(&path, &store.cwd);
            let user = store.load_file(&path, true)?;
            let merged = store.merge_with_default(user);
            let mut r = Resolved::new(merged, Some(&path), &store.opts, &store.cop_names);
            r.unregistered = store.unregistered(&r.hash);
            store.forced = Some(Arc::new(r));
        }
        Ok(store)
    }

    /// Keys of a merged config that name no registered cop (kept when unrecognized cops are
    /// ignored); Registry.qualified_cop? is false for them.
    fn unregistered(&self, hash: &Map) -> Vec<String> {
        hash.keys()
            .filter(|k| {
                k.contains('/') && !self.default.contains(k) && !k.split('/').all(|p| self.plugin_constants.contains(p))
            })
            .map(String::from)
            .collect()
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// A config so far loaded `require:`s Ruby code rbcop cannot load (custom cops, or a plugin
    /// gem it could not integrate).
    /// ConfigLoader.loaded_plugins and loaded_features: every `plugins:`/`require:` name loaded so far.
    pub fn loaded_plugins(&self) -> &[String] {
        &self.loaded_plugins
    }

    pub fn requires_ruby_code(&self) -> bool {
        self.custom_cops
    }

    /// The loaded plugins rbcop does not implement that define no cops.
    pub fn copless_plugins(&self) -> Vec<&str> {
        self.copless_plugins.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// A file one of those plugins brings in (its `AllCops: Include` matches it, as rubocop-erb's
    /// `**/*.erb`): what RuboCop inspects there comes from the plugin's Ruby code (ERB extraction),
    /// so rbcop runs none of its cops on it.
    // ponytail: the Include patterns stand in for the plugin's own test (rubocop-erb: the `.erb`
    // extension), which is Ruby code rbcop cannot read.
    pub fn copless_plugin_file(&self, cfg: &Resolved, file: &Path) -> bool {
        let (rel, abs) = (cfg.relative(file), file.display().to_string());
        self.copless_plugins.iter().flat_map(|(_, pats)| pats).any(|p| p.matches(&rel) || p.matches(&abs))
    }

    /// The qualified `--only` list, known once configs (and so plugins) have loaded: Registry#enabled?
    /// enables those cops outright, so every config resolved so far is re-resolved with it.
    pub fn set_only(&mut self, only: Vec<String>) {
        if self.opts.only == only {
            return;
        }
        self.opts.only = only;
        for r in self.resolved.values_mut().chain(self.forced.iter_mut()) {
            let r = Arc::make_mut(r);
            r.opts = self.opts.clone();
            for &name in &self.cop_names {
                let cfg = r.build_cop(name, &self.opts);
                r.cops.insert(name.to_string(), cfg);
            }
        }
    }

    /// ConfigLoader.default_configuration: default.yml with the plugins loaded so far merged in.
    pub fn default_config(&self) -> Arc<Resolved> {
        let opts = Options { force_default_config: true, ..Options::default() };
        Arc::new(Resolved::new(apply_default_overrides(&self.default, &opts), None, &opts, &self.cop_names))
    }

    /// The CLI applies `--color`/`--no-color` once the pwd config has loaded.
    pub fn set_color(&mut self, on: bool) {
        self.opts.color = on;
    }

    /// Config-loading output goes to stderr as it happens, as RuboCop's `warn` does.
    fn emit(&mut self, out: &[(bool, String)]) {
        eprint!("{}", validator::render(out, self.opts.color));
        self.emitted.extend_from_slice(out);
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
            // ConfigLoader.apply_default_overrides: only the command line can turn preview on here.
            None => apply_default_overrides(&self.default, &self.opts),
            Some(path) => {
                let mut user = self.load_file(path, true)?;
                validator::check_target_ruby(&user, path, &self.cwd)?;
                if !self.opts.ignore_parent_exclusion {
                    self.add_excludes_from_files(&mut user, path)?;
                }
                self.merge_with_default(user)
            }
        };
        let mut r = Resolved::new(hash, file.as_deref(), &self.opts, &self.cop_names);
        r.unregistered = self.unregistered(&r.hash);
        let r = Arc::new(r);
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

    /// ConfigLoader.load_file (§2); `check` runs Config#check (validation).
    // ponytail: a cached load replays the first load's warnings; a later load in RuboCop sees the
    // plugins loaded since, which can only make fewer cop names unrecognized.
    fn load_file(&mut self, path: &Path, check: bool) -> Result<Map, String> {
        if let Some((m, out)) = self.files.get(path).cloned() {
            self.emit(&out);
            return Ok(m);
        }
        let start = self.emitted.len();
        let text = std::fs::read_to_string(path).map_err(|e| match e.kind() {
            // ConfigLoader.read_file's ConfigNotFoundError.
            std::io::ErrorKind::NotFound => format!("Configuration file not found: {}", path.display()),
            _ => format!("{}: {e}", path.display()),
        })?;
        if text.contains("<%") {
            self.warnings.push(format!("{}: ERB in config is not supported; parsed as plain YAML", path.display()));
        }
        let mut hash = match parse_yaml_file(&text, &path.display().to_string())? {
            Value::Map(m) => Arc::unwrap_or_clone(m),
            Value::Null => Map::default(),
            // A ValidationError, which the CLI prints as `Error: ...`.
            _ => return Err(format!("Error: Malformed configuration in {}", path.display())),
        };
        // Plugins (and plugin-capable `require:` gems) only matter here for their default configs.
        let mut plugins: Vec<(String, Load)> = Vec::new();
        let mut migrate = Vec::new();
        for key in ["plugins", "require"] {
            for entry in seq(hash.remove(key)) {
                match entry {
                    Value::Str(mut s) => {
                        // ConfigLoaderResolver#resolve_requires: a plugin-capable `require:`
                        // entry (Plugin.plugin_capable?: RuboCop's built-in plugin, or a gem of
                        // that name whose gemspec names a `default_lint_roller_plugin`) warns, then
                        // loads as a plugin; any other entry is a plain `require` (FeatureLoader).
                        let builtin = s == INTERNAL_AFFAIRS || s == OBSOLETE_INTERNAL_AFFAIRS;
                        let local = s.starts_with('.') || s.starts_with('/');
                        let capable = key == "plugins"
                            || builtin
                            || crate::helpers::cop_registry::native_plugin(&s).is_some()
                            || (!local && self.feature(&s).is_ok_and(|f| f.plugin));
                        if key == "require" && capable {
                            migrate.push((plugins.len(), true, format!(
                                "{s} extension supports plugin, specify `plugins: {s}` instead of `require: {s}` in {}.\nFor more information, see https://docs.rubocop.org/rubocop/plugin_migration_guide.html.\n",
                                path.display()
                            )));
                        }
                        // Plugin::Loader.normalize
                        if s == OBSOLETE_INTERNAL_AFFAIRS {
                            migrate.push((
                                plugins.len(),
                                true,
                                format!(
                                    "Specify `{INTERNAL_AFFAIRS}` instead of `{OBSOLETE_INTERNAL_AFFAIRS}` in your configuration.\n"
                                ),
                            ));
                            s = INTERNAL_AFFAIRS.to_string();
                        }
                        plugins.push((s, if capable { Load::Plugin } else { Load::Require }))
                    }
                    // Plugin::Loader.normalize: `{name => {enabled:, plugin_class_name:}}`.
                    Value::Map(m) => {
                        for (name, config) in &m.0 {
                            let config = config.as_map();
                            if config.and_then(|c| c.get("enabled")) == Some(&Value::Bool(false)) {
                                continue;
                            }
                            let named = config.and_then(|c| c.get("plugin_class_name")).and_then(Value::as_string);
                            plugins.push((name.clone(), named.map_or(Load::Plugin, Load::NamedPlugin)));
                        }
                    }
                    _ => {}
                }
            }
        }
        // Local files are custom cops rbcop cannot load.
        self.custom_cops |= plugins.iter().any(|(n, _)| n.starts_with('.') || n.starts_with('/'));
        // Config validation knows their class and module names (`Style/MiddleDot` defined in
        // `require: ../lib/linter/rubocop_middle_dot`), as a plugin gem's lib/.
        let mut local = Vec::new();
        let mut missing = None;
        for (i, (n, _)) in plugins.iter().enumerate().filter(|(_, (n, _))| n.starts_with('.') || n.starts_with('/')) {
            // FeatureLoader#load: `require` of the path (`.rb` added unless given), then of the
            // path with `-` as `/`; the LoadError names the first.
            let base = path.parent().unwrap_or(Path::new("."));
            let at = |n: &str| if n.starts_with('/') { PathBuf::from(n) } else { base.join(n) };
            let rb = |p: PathBuf| match p.extension() {
                Some(e) if e == "rb" => p,
                _ => PathBuf::from(format!("{}.rb", p.display())),
            };
            let Some(file) = [rb(at(n)), rb(at(&n.replace('-', "/")))].into_iter().find(|f| f.is_file()) else {
                missing.get_or_insert_with(|| (i, format!("cannot load such file -- {}", at(n).display())));
                continue;
            };
            ruby_file_constants(&file, &mut self.plugin_constants);
            local.push(file);
        }
        // The first entry that fails to load ends the run, after the warnings of the entries
        // before it.
        let names: Vec<String> = plugins.iter().map(|(n, _)| n.clone()).collect();
        let failed = match self.integrate_plugins(plugins) {
            Err(e) => {
                let at = self.failed_entry.take().and_then(|f| names.iter().position(|n| *n == f));
                match (at, missing) {
                    (Some(at), Some((m, local))) if m < at => Some((m, local)),
                    (at, _) => Some((at.unwrap_or(0), e)),
                }
            }
            Ok(()) => missing,
        };
        // ponytail: a gem listed after a missing local file still integrates before the error.
        let migrate: validator::Output = match failed {
            Some((at, e)) => {
                let out: validator::Output =
                    migrate.into_iter().filter(|(i, ..)| *i < at).map(|(_, b, s)| (b, s)).collect();
                self.emit(&out);
                return Err(e);
            }
            None => migrate.into_iter().map(|(_, b, s)| (b, s)).collect(),
        };
        // ConfigLoader#load_file resolves `plugins:` and then `require:`: the cop classes a local
        // file defines register once it loads.
        // ponytail: every local file registers after every gem of this config file, not at its place
        // in `require:`; that order only matters for a cop name two departments share.
        // Registry: a cop is a class descending from a cop base, not excluded from the registry
        // (ruby_cops). Only a file in which none is found leaves the registry open.
        // ponytail: files the local file itself requires are not read.
        for file in local {
            let cops = ruby_cops_in(vec![file.clone()]);
            if cops.is_empty() {
                self.open_custom = true;
                ruby_file_constants(&file, &mut self.open_constants);
            }
            crate::helpers::cop_registry::register_custom(cops);
        }
        crate::helpers::cop_registry::set_open(self.open_custom, &self.open_constants);
        self.emit(&migrate);
        if let Some(Value::Map(gems)) = hash.remove("inherit_gem") {
            let mut inherit = seq(hash.remove("inherit_from"));
            for (gem, paths) in Arc::unwrap_or_clone(gems).0 {
                // ConfigLoaderResolver#resolve_inheritance_from_gems's ArgumentError.
                if gem == "rubocop" {
                    return Err("can't inherit configuration from the rubocop gem".into());
                }
                let dir = self.gem_dir(&gem)?;
                for p in seq(Some(paths)).into_iter().rev() {
                    if let Some(p) = p.as_string() {
                        inherit.insert(0, Value::Str(dir.join(p).display().to_string()));
                    }
                }
            }
            hash.insert("inherit_from", Value::Seq(inherit.into()));
        }
        self.resolve_inheritance(path, &mut hash)?;
        hash.remove("inherit_from");
        self.add_missing_namespaces(path, &mut hash)?;
        if check {
            let mut out = Vec::new();
            let checked = validator::Validator {
                default: &self.default,
                pwd: &self.cwd,
                loaded_plugins: &self.loaded_plugins,
                ignore_unrecognized_cops: self.opts.ignore_unrecognized_cops,
                custom_cops: self.custom_cops,
                plugin_constants: &self.plugin_constants,
                color: self.opts.color,
            }
            .check(&mut hash, path, &mut out);
            self.emit(&out);
            checked?;
            // Config#make_excludes_absolute calls `map!` on every Exclude: RuboCop dies with a
            // NoMethodError (and a backtrace rbcop does not print) on anything but an Array.
            for (_, section) in &hash.0 {
                let Some(exclude) = section.as_map().and_then(|m| m.get("Exclude")) else { continue };
                let receiver = match exclude {
                    Value::Seq(_) | Value::Null => continue,
                    Value::Bool(b) => b.to_string(),
                    Value::Str(_) => "an instance of String".into(),
                    Value::Sym(_) => "an instance of Symbol".into(),
                    Value::Int(_) => "an instance of Integer".into(),
                    Value::Float(_) => "an instance of Float".into(),
                    Value::Regex(_) => "an instance of Regexp".into(),
                    Value::Map(_) => "an instance of Hash".into(),
                };
                return Err(format!("undefined method 'map!' for {receiver}"));
            }
        }
        make_excludes_absolute(&mut hash, &base_dir_for(Some(path)));
        let out = self.emitted[start..].to_vec();
        self.files.insert(path.to_path_buf(), (hash.clone(), out));
        Ok(hash)
    }

    fn resolve_inheritance(&mut self, path: &Path, hash: &mut Map) -> Result<(), String> {
        let dir = path.parent().unwrap_or(Path::new("/"));
        let mut bases = Vec::new();
        // ConfigLoaderResolver#base_configs: a glob entry is expanded with Dir.glob (from the
        // working directory) and each match is then taken relative to this file's directory.
        let entries: Vec<String> = seq(hash.get("inherit_from").cloned())
            .iter()
            .filter_map(Value::as_string)
            .flat_map(|e| if glob::is_glob(&e) { glob::dir_glob(&e, &self.cwd) } else { vec![e] })
            .collect();
        for entry in entries {
            if entry.starts_with("http://") || entry.starts_with("https://") {
                self.warnings.push(format!("{}: remote inherit_from {entry} is not supported", path.display()));
                continue;
            }
            let base_path = expand_path(Path::new(&entry), dir);
            if base_path == path {
                continue;
            }
            // RuboCop recurses until its stack overflows; report the cycle instead.
            if self.loading.contains(&base_path) {
                return Err(format!("{}: inherit_from cycle through {}", path.display(), base_path.display()));
            }
            self.loading.push(base_path.clone());
            let base = self.load_file(&base_path, true);
            self.loading.pop();
            bases.push((base_path, base?));
        }
        for (base_path, base) in bases.into_iter().rev() {
            override_department_setting_for_cops(&base, hash);
            override_enabled_for_disabled_departments(&base, hash);
            for (k, v) in base.0 {
                let Value::Map(v) = v else { continue };
                let only_base_has_include = v.contains("Include") && !hash.map(&k).is_some_and(|m| m.truthy("Include"));
                let merged = match hash.map(&k) {
                    Some(derived) => {
                        let mode = determine_inherit_mode(hash, &k, self.opts.preview);
                        merge(&v, derived, &mode, false)
                    }
                    None => Arc::unwrap_or_clone(v),
                };
                hash.insert(k.clone(), Value::Map(merged.into()));
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
                        m.insert("Include", Value::Seq(rebased.into()));
                    }
                }
            }
        }
        Ok(())
    }

    /// ConfigLoader.add_missing_namespaces: every key but an obsolete cop name goes through
    /// Registry.qualified_cop_name(key, path), which warns about a bare cop name (`Debugger:`
    /// becomes `Lint/Debugger:`) and a wrong department (`Style/LineLength:` becomes
    /// `Layout/LineLength:`) and raises AmbiguousCopName for a bare name two departments share.
    /// A requalified key moves to the end (`hash[q] = hash.delete(key)`).
    fn add_missing_namespaces(&mut self, path: &Path, hash: &mut Map) -> Result<(), String> {
        let legacy = validator::legacy_cop_names();
        let shown = validator::smart_path(path, &self.cwd);
        let origin = path.display().to_string();
        let keys: Vec<String> = hash.keys().filter(|k| !legacy.contains(&k.to_string())).map(String::from).collect();
        let mut warnings = Vec::new();
        let mut result = Ok(());
        for key in keys {
            match crate::helpers::cop_registry::qualify(&key, &origin, Some(&shown), true, &mut warnings) {
                Ok(q) if q != key => {
                    let value = hash.remove(&key).unwrap_or(Value::Null);
                    hash.insert(q, value);
                }
                Ok(_) => {}
                Err(e) => {
                    result = Err(format!("Error: {e}"));
                    break;
                }
            }
        }
        let out: validator::Output = warnings.into_iter().map(|w| (false, w)).collect();
        self.emit(&out);
        result
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
        let highest = self.load_file(&top, true)?;
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
        all.insert("Exclude", Value::Seq(excludes.into()));
        Ok(())
    }

    /// Plugin::ConfigurationIntegrator: fold each plugin gem's config/default.yml into the defaults.
    // ponytail: assumes the LintRoller rules path is <gem>/config/default.yml, which every
    // rubocop-* plugin checked so far uses; local `require: ./file` entries (custom cops) are skipped.
    fn integrate_plugins(&mut self, names: Vec<(String, Load)>) -> Result<(), String> {
        let names: Vec<(String, Load)> = names
            .into_iter()
            .filter(|(n, _)| !n.starts_with('.') && !n.starts_with('/') && !self.loaded_plugins.contains(n))
            .collect();
        if names.is_empty() {
            return Ok(());
        }
        // Plugin::Loader.load requires every plugin (registering its cops) before
        // ConfigurationIntegrator loads ConfigLoader.default_configuration, whose qualified keys
        // fill Registry#unqualified_cop_names, and then each plugin's config.
        // ponytail: a config's `plugins:` and plugin-capable `require:` gems load as one batch;
        // RuboCop integrates each such `require:` on its own, after `plugins:`.
        let mut configs: Vec<Result<&'static crate::helpers::cop_registry::NativePlugin, PathBuf>> = Vec::new();
        for (name, load) in names {
            self.loaded_plugins.push(name.clone());
            if let Some(native) = crate::helpers::cop_registry::native_plugin(&name) {
                // A plugin rbcop implements: its cops register and its vendored default.yml merges,
                // so the gem need not be installed.
                crate::helpers::cop_registry::register_plugin(native);
                configs.push(Ok(native));
                continue;
            }
            // RuboCop's built-in plugin (Plugin::BUILTIN_INTERNAL_PLUGINS) lives in the rubocop
            // gem: its cops in lib/rubocop/cop/internal_affairs/, its config in
            // config/internal_affairs.yml.
            let (found, config, lib) = if name == INTERNAL_AFFAIRS {
                let rubocop = self.gem_dir("rubocop").map(|d| Feature { dir: Some(d), file: None, plugin: true });
                (rubocop, "config/internal_affairs.yml", "lib/rubocop/cop/internal_affairs")
            } else {
                (self.feature(&name), "config/default.yml", "lib")
            };
            let feature = match found {
                Ok(f) => f,
                Err(e) if e.starts_with("cannot load such file") => {
                    self.failed_entry = Some(name);
                    return Err(e);
                }
                Err(e) => {
                    self.warnings.push(e);
                    // Its cops are unknown here.
                    self.custom_cops = true;
                    self.open_custom = true;
                    continue;
                }
            };
            // Plugin::Loader.constantize: once its feature loads, a plugin's class comes from a
            // Hash entry's `plugin_class_name` or from the `default_lint_roller_plugin` metadata
            // of the gem of that name; without either the run fails (Plugin::LoadError).
            if load == Load::Plugin && !feature.plugin {
                self.failed_entry = Some(name.clone());
                return Err(format!(
                    "Error: Failed to load plugin `{name}` because the corresponding plugin class could not be determined \
                     for instantiation.\nTry upgrading it first (e.g., `bundle update {name}`).\nIf `{name}` is not yet \
                     a plugin, use `require: {name}` instead of `plugins: {name}` in your configuration.\n\nFor further \
                     assistance, check with the developer regarding the following points:\n\
                     https://docs.rubocop.org/rubocop/plugin_migration_guide.html"
                ));
            }
            // A Ruby library outside any gem (`require: set`): no cops.
            let Some(dir) = feature.dir else { continue };
            let lib = dir.join(lib);
            // Kernel.const_get of the name, then `.new(plugin_config)` on it.
            // ponytail: the constant is looked for in the gem's lib/ only.
            if let Load::NamedPlugin(class) = &load {
                let problem = match ruby_constant_kinds(&ruby_files(&lib)).get(class.trim_start_matches("::")) {
                    Some(true) => None,
                    Some(false) => Some(format!("undefined method 'new' for module {class}")),
                    None => Some(format!(
                        "Failed while configuring plugin `{name}': no constant with name `{class}' was found."
                    )),
                };
                if let Some(problem) = problem {
                    self.failed_entry = Some(name.clone());
                    return Err(problem);
                }
            }
            let path = dir.join(config);
            // A plugin loads its gem (its cops, its config); a plain `require` loads the files
            // it reaches, and the gem's config/default.yml only when that code injects it into
            // the default configuration (an extension's Inject.defaults!).
            let (files, merge) = if load == Load::Require {
                let rubocop = self.gem_dir("rubocop").ok();
                let files = match &feature.file {
                    Some(file) => required_files(&lib, file, rubocop.as_deref()),
                    None => ruby_files(&lib),
                };
                let inject = path.is_file() && injects_defaults(&files);
                (files, inject)
            } else {
                (ruby_files(&lib), path.is_file())
            };
            for file in &files {
                ruby_file_constants(file, &mut self.plugin_constants);
            }
            let cops = ruby_cops_in(files.clone());
            if !merge && cops.is_empty() {
                // A plugin without config/default.yml may still register cops; the files a plain
                // `require` reaches were read, and define none.
                if load != Load::Require && !path.is_file() {
                    self.custom_cops = true;
                    self.open_custom = true;
                    for file in &files {
                        ruby_file_constants(file, &mut self.open_constants);
                    }
                }
                continue;
            }
            // RuboCop's registry holds the cop classes the gem defines, whether or not its
            // default.yml configures them (and not the entries without a class). Classes
            // configured there come in that order, then the others. Other class and module
            // names (helpers, the plugin class, namespaces) are not cops.
            let config = if merge {
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                match parse_yaml_file(&text, &path.display().to_string())? {
                    Value::Map(m) => Arc::unwrap_or_clone(m),
                    _ => Map::default(),
                }
            } else {
                Map::default()
            };
            let keys: Vec<String> = config.keys().map(String::from).collect();
            if cops.is_empty() {
                for file in &files {
                    ruby_file_constants(file, &mut self.open_constants);
                }
            }
            let names: Vec<String> = if cops.is_empty() {
                keys.into_iter().filter(|k| k.contains('/')).collect()
            } else {
                let mut names: Vec<String> =
                    keys.iter().filter(|k| cops.iter().any(|(c, _)| c == *k)).cloned().collect();
                names.extend(cops.iter().map(|(c, _)| c.clone()).filter(|c| !keys.contains(c)));
                names
            };
            let before = crate::helpers::cop_registry::REGISTRY.len();
            crate::helpers::cop_registry::register_foreign(names);
            // A plugin with no cops of its own (rubocop-erb) acts through its Ruby code; a plain
            // `require` never counts as one.
            if load != Load::Require && cops.is_empty() && crate::helpers::cop_registry::REGISTRY.len() == before {
                let include = config.map("AllCops").and_then(|a| patterns(a, "Include")).unwrap_or_default();
                self.copless_plugins.push((name.clone(), include));
            }
            crate::helpers::cop_registry::set_foreign_autocorrect(
                cops.into_iter().filter_map(|(c, ac)| ac.then_some(c)),
            );
            if merge {
                configs.push(Err(path));
            }
        }
        // (A gem with a config is a plugin, or an older extension whose inject_defaults! loads
        // the default configuration as well.)
        if !configs.is_empty() {
            crate::helpers::cop_registry::fill_unqualified_cop_names();
        }
        let mut combined = self.default.clone();
        let mut configured: Vec<String> = Vec::new();
        for config in configs {
            let mut plugin = match config {
                Ok(native) => {
                    // Path parameters there resolve against the working directory, as for any
                    // config file not named .rubocop*.
                    let label = format!("{}/config/default.yml", native.gem);
                    let Value::Map(m) = parse_yaml_file(native.default_yml, &label)? else { continue };
                    let mut m = Arc::unwrap_or_clone(m);
                    make_excludes_absolute(&mut m, &self.cwd);
                    m
                }
                Err(path) => self.load_file(&path, false)?,
            };
            if let Some(Value::Map(new_all)) = plugin.get("AllCops").cloned() {
                let mut all = combined.map("AllCops").cloned().unwrap_or_default();
                for (key, value) in Arc::unwrap_or_clone(new_all).0 {
                    match (all.get(&key), &value) {
                        (Some(Value::Seq(existing)), Value::Seq(added)) => {
                            let mut union = Vec::clone(existing);
                            union.extend(added.iter().filter(|v| !existing.contains(v)).cloned());
                            all.insert(key.clone(), Value::Seq(union.into()));
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
                plugin.insert("AllCops", Value::Map(all.into()));
            }
            combined = merge_with_default(&combined, plugin, false, &self.opts);
        }
        if let (Some(all), Some(Value::Map(new_all))) =
            (self.default.map_mut("AllCops"), combined.get("AllCops").cloned())
        {
            for (k, v) in Arc::unwrap_or_clone(new_all).0 {
                all.insert(k, v);
            }
        }
        for (k, v) in combined.0 {
            if k == "AllCops" {
                continue;
            }
            let merged = match (self.default.get(&k), &v) {
                (Some(Value::Map(d)), Value::Map(p)) => Value::Map(merge(d, p, &Map::default(), false).into()),
                _ => v,
            };
            self.default.insert(k, merged);
        }
        Ok(())
    }

    /// ConfigLoaderResolver#merge_with_default (§4) against the current defaults.
    fn merge_with_default(&self, config: Map) -> Map {
        // It loads ConfigLoader.default_configuration, whose `AllCops` key memoizes
        // Registry#unqualified_cop_names (before any subdirectory config loads its plugins).
        crate::helpers::cop_registry::fill_unqualified_cop_names();
        merge_with_default(&self.default, config, true, &self.opts)
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
            .args([
                "-e",
                "begin; print Gem::Specification.find_by_name(ARGV[0]).gem_dir; \
                 rescue Gem::LoadError => e; $stderr.print e.message; exit 1; end",
                "--",
                gem,
            ])
            .current_dir(&self.cwd)
            .output()
            .map_err(|e| format!("inherit_gem {gem}: cannot run ruby: {e}"))?;
        if !out.status.success() {
            // gem_config_path's Gem::LoadError.
            let message = String::from_utf8_lossy(&out.stderr);
            return Err(format!("Unable to find gem {gem}; is the gem installed? {}", message.trim_end()));
        }
        let dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).into_owned());
        self.gem_dirs.insert(gem.to_string(), dir.clone());
        Ok(dir)
    }

    /// What a `plugins:`/`require:` feature loads: FeatureLoader#load's `require` of the feature,
    /// then of it with `-` as `/` (`rubocop/require_tools` is in rubocop-require_tools). Err
    /// `cannot load such file -- <feature>` (RuboCop's LoadError) when Ruby finds neither.
    fn feature(&mut self, feature: &str) -> Result<Feature, String> {
        if let Some(f) = self.features.get(feature) {
            return f.clone();
        }
        let found = self.find_feature(feature);
        self.features.insert(feature.to_string(), found.clone());
        found
    }

    fn find_feature(&self, feature: &str) -> Result<Feature, String> {
        // Features come from config files; anything else must never reach the ruby command line.
        if feature.is_empty()
            || feature.starts_with('-')
            || !feature.chars().all(|c| c.is_ascii_alphanumeric() || "_-./".contains(c))
        {
            return Err(format!("cannot load such file -- {feature}"));
        }
        // Prints whether the gem named `f` is a lint_roller plugin, then the gem dir and the
        // file the feature resolves to (none for the standard library).
        let out = std::process::Command::new("ruby")
            .args([
                "-e",
                "f = ARGV[0]; plugin = begin; !!Gem::Specification.find_by_name(f).metadata['default_lint_roller_plugin']; \
                 rescue Gem::LoadError, StandardError; false; end; \
                 [f, f.tr('-', '/')].uniq.each do |t| \
                 spec = Gem::Specification.find_by_path(t); \
                 (puts plugin, spec.gem_dir, spec.full_require_paths.flat_map { |d| [\"#{d}/#{t}.rb\", \"#{d}/#{t}\"] }.find { |p| File.file?(p) }; exit) if spec && !spec.default_gem?; \
                 (puts plugin; exit) if spec || $LOADED_FEATURES.include?(t) || $LOADED_FEATURES.include?(\"#{t}.rb\") || \
                 (($LOAD_PATH.resolve_feature_path(t) rescue nil)); \
                 end; $stderr.print \"cannot load such file -- #{f}\"; exit 1",
                "--",
                feature,
            ])
            .current_dir(&self.cwd)
            .output()
            .map_err(|e| format!("{feature}: cannot run ruby: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim_end().to_string());
        }
        let out = String::from_utf8_lossy(&out.stdout);
        let mut lines = out.lines();
        let plugin = lines.next() == Some("true");
        let mut path = || lines.next().filter(|l| !l.is_empty()).map(PathBuf::from);
        Ok(Feature { plugin, dir: path(), file: path() })
    }
}

/// How a `plugins:`/`require:` entry loads.
#[derive(Clone, PartialEq)]
enum Load {
    /// Plugin::Loader, with the plugin class from the gemspec metadata.
    Plugin,
    /// Plugin::Loader, with this `plugin_class_name`.
    NamedPlugin(String),
    /// FeatureLoader: a plain `require`.
    Require,
}

/// A feature found by Store::feature.
#[derive(Clone)]
struct Feature {
    /// The gem of the feature's name is a lint_roller plugin (`default_lint_roller_plugin`).
    plugin: bool,
    /// The installed gem it is in; None for the standard library (or a default gem).
    dir: Option<PathBuf>,
    /// The Ruby file it resolves to.
    file: Option<PathBuf>,
}

/// The Ruby files a plain `require` of `file` loads: the file and, transitively, those its
/// literal `require_relative`s and its `require`s of features under the gem's `lib` name. Files
/// of the rubocop gem itself (`loaded`) are loaded before any config is read, so requiring them
/// again does nothing. A `require` of a computed path (`Dir[...].each { |f| require f }`) is taken
/// to load every file under `lib`.
// ponytail: only literal require lines are followed; conditions on them (`if defined?(...)`) and
// requires into other gems are not.
fn required_files(lib: &Path, file: &Path, loaded: Option<&Path>) -> Vec<PathBuf> {
    static LITERAL: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r##"^\s*(require|require_relative)\b[\s(]*['"]([^'"#]+)['"]"##).unwrap()
    });
    static COMPUTED: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r##"^[^#]*\brequire(?:_relative)?\b[\s(]*(?:[a-z_]\w*\s*(?:[)};]|$)|File\.|"[^"]*#\{)"##).unwrap()
    });
    let rb = |p: PathBuf| match p.extension() {
        Some(e) if e == "rb" => p,
        _ => PathBuf::from(format!("{}.rb", p.display())),
    };
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut todo = vec![file.to_path_buf()];
    while let Some(f) = todo.pop() {
        if seen.contains(&f) || loaded.is_some_and(|l| f.starts_with(l)) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        for line in text.lines() {
            if let Some(m) = LITERAL.captures(line) {
                let target =
                    if &m[1] == "require_relative" { f.parent().unwrap().join(&m[2]) } else { lib.join(&m[2]) };
                todo.push(rb(normalize(&target)));
            } else if COMPUTED.is_match(line) {
                return ruby_files(lib).into_iter().filter(|f| !loaded.is_some_and(|l| f.starts_with(l))).collect();
            }
        }
        seen.push(f);
    }
    seen
}

/// Loading these files sets ConfigLoader's default configuration: one of them calls an
/// extension's `Inject.defaults!` at the top level, and one sets the default configuration.
/// (Code that sets it only from a method RuboCop calls later, as standard's plugin, does not.)
// ponytail: a name match for the call and the assignment, not a trace of which method is called.
fn injects_defaults(files: &[PathBuf]) -> bool {
    static CALL: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"(?m)^[A-Z][\w:]*\.(?:inject_)?defaults!").unwrap());
    static SET: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"@default_configuration\b|\bdefault_configuration\s*=[^=]").unwrap());
    let texts: Vec<String> = files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
    texts.iter().any(|t| CALL.is_match(t)) && texts.iter().any(|t| SET.is_match(t))
}

/// Plugin::INTERNAL_AFFAIRS_PLUGIN_NAME, RuboCop's built-in plugin, and the obsolete name it
/// is also required by.
const INTERNAL_AFFAIRS: &str = "rubocop-internal_affairs";
const OBSOLETE_INTERNAL_AFFAIRS: &str = "rubocop/cop/internal_affairs";

/// ConfigLoaderResolver#preview?: the command line, else the hash's own `AllCops: Preview: true`.
fn preview_on(config: &Map, cli: Option<bool>) -> bool {
    cli.unwrap_or_else(|| config.map("AllCops").and_then(|a| a.get("Preview")) == Some(&Value::Bool(true)))
}

/// ConfigLoaderResolver#apply_preview_defaults: a cop's `Preview` hash replaces its defaults under
/// preview; the `Preview` key is dropped either way.
pub fn apply_preview_defaults(default: &Map, preview: bool) -> Map {
    let mut out = default.clone();
    for (_, v) in out.0.iter_mut() {
        let Value::Map(params) = v else { continue };
        let params = Arc::make_mut(params);
        if !matches!(params.get("Preview"), Some(Value::Map(_))) {
            continue;
        }
        let Some(Value::Map(preview_params)) = params.remove("Preview") else { continue };
        if preview {
            for (k, x) in Arc::unwrap_or_clone(preview_params).0 {
                params.insert(k, x);
            }
        }
    }
    out
}

/// ConfigLoaderResolver#with_preview_exclude_merge: under preview `Exclude` is merged, unless
/// inherit_mode names it either way.
fn with_preview_exclude_merge(mut mode: Map, config: &Map, cli: Option<bool>) -> Map {
    let lists = |m: &Map, which: &str| m.strings(which).iter().any(|k| k == "Exclude");
    if !preview_on(config, cli) || lists(&mode, "override") || lists(&mode, "merge") {
        return mode;
    }
    let mut merge = seq(mode.get("merge").cloned());
    merge.push(Value::Str("Exclude".into()));
    mode.insert("merge", Value::Seq(merge.into()));
    mode
}

/// ConfigLoader.apply_default_overrides: the default configuration used as is (no config file, or
/// --force-default-config), with --preview and --enable-all-cops / --disable-all-cops applied.
fn apply_default_overrides(default: &Map, opts: &Options) -> Map {
    // ConfigLoader.default_configuration memoizes Registry#unqualified_cop_names (merge_with_default).
    crate::helpers::cop_registry::fill_unqualified_cop_names();
    let mut hash = apply_preview_defaults(default, opts.preview == Some(true));
    if let Some(enable) = opts.all_cops {
        for (_, v) in hash.0.iter_mut() {
            if let Value::Map(m) = v {
                Arc::make_mut(m).insert("Enabled", Value::Bool(enable));
            }
        }
    }
    hash
}

/// ConfigLoaderResolver#merge_with_default (§4).
fn merge_with_default(base_default: &Map, mut config: Map, unset_nil: bool, opts: &Options) -> Map {
    let cli_preview = opts.preview;
    let all = config.map("AllCops").cloned().unwrap_or_default();
    // resolve_default_overrides: --enable-all-cops / --disable-all-cops win over the config.
    let (dbd, ebd) = match opts.all_cops {
        Some(enable) => (!enable, enable),
        None => (all.truthy("DisabledByDefault"), all.truthy("EnabledByDefault")),
    };
    let base_default = &apply_preview_defaults(base_default, preview_on(&config, cli_preview));
    let mut default = base_default.clone();
    if dbd || ebd {
        for (_, v) in default.0.iter_mut() {
            if let Value::Map(m) = v {
                Arc::make_mut(m).insert("Enabled", Value::Bool(!dbd));
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
                    Arc::make_mut(p).insert("Enabled", orig.clone());
                }
            }
        }
        for (_, v) in config.0.iter_mut() {
            if let Value::Map(m) = v
                && !m.contains("Enabled")
            {
                Arc::make_mut(m).0.insert(0, ("Enabled".into(), Value::Bool(true)));
            }
        }
    }
    override_enabled_for_disabled_departments(&default, &mut config);
    let mode =
        with_preview_exclude_merge(config.map("inherit_mode").cloned().unwrap_or_default(), &config, cli_preview);
    merge(&default, &config, &mode, unset_nil)
}

/// The Ruby files under a gem's lib/, sorted.
fn ruby_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rb") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    files.sort();
    files
}

/// Class and module names a Ruby file defines: a plugin registers cops its config/default.yml
/// need not list, and RuboCop's registry knows them.
// ponytail: a name match, not a registry; `Dept/Cop` counts as known when both are defined.
fn ruby_file_constants(path: &Path, out: &mut std::collections::HashSet<String>) {
    static RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"(?m)^\s*(?:class|module)\s+([A-Z]\w*)").unwrap());
    if let Ok(text) = std::fs::read_to_string(path) {
        out.extend(RE.captures_iter(&text).map(|c| c[1].to_string()));
    }
}

/// A `module`/`class` line: indentation, keyword, name, superclass, `; ...` rest.
static SCOPE_OPEN: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r"^(\s*)(module|class)\s+((?:::)?[A-Z]\w*(?:::[A-Z]\w*)*)\s*(?:<\s*((?:::)?[A-Z][\w:]*))?\s*(;.*)?(?:#.*)?$",
    )
    .unwrap()
});
static SCOPE_END: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^(\s*)end\b").unwrap());

/// The fully qualified class and module names some Ruby files define, each with whether it is a
/// class (Kernel.const_get of a `plugin_class_name`). Nesting as in `ruby_cops_in`.
fn ruby_constant_kinds(files: &[PathBuf]) -> HashMap<String, bool> {
    let mut out = HashMap::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        let mut stack: Vec<(usize, Vec<String>)> = Vec::new();
        for line in text.lines() {
            if let Some(m) = SCOPE_OPEN.captures(line) {
                let mut path = if m[3].starts_with("::") {
                    Vec::new()
                } else {
                    stack.last().map(|s| s.1.clone()).unwrap_or_default()
                };
                path.extend(m[3].trim_start_matches("::").split("::").map(String::from));
                let class = out.entry(path.join("::")).or_insert(false);
                *class |= &m[2] == "class";
                if !m.get(5).is_some_and(|t| t.as_str().contains("end")) {
                    stack.push((m[1].len(), path));
                }
            } else if let Some(m) = SCOPE_END.captures(line)
                && stack.last().is_some_and(|s| s.0 == m[1].len())
            {
                stack.pop();
            }
        }
    }
    out
}

/// The cops some Ruby files define (Badge.for of each class under RuboCop::Cop that descends
/// from a cop base class and does not call `exclude_from_registry`), sorted, each with whether it
/// supports autocorrect (`extend AutoCorrector`, its own or a superclass's).
// ponytail: reads nesting from `module`/`class` lines and the `end` at the same indentation, as
// every rubocop-* gem is formatted; classes opened by metaprogramming are not seen.
fn ruby_cops_in(files: Vec<PathBuf>) -> Vec<(String, bool)> {
    struct Class {
        path: Vec<String>,
        superclass: String,
        autocorrect: bool,
        excluded: bool,
    }
    let mut classes: Vec<Class> = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        // (indent, path of this scope, index in `classes` for a class)
        let mut stack: Vec<(usize, Vec<String>, Option<usize>)> = Vec::new();
        for line in text.lines() {
            if let Some(m) = SCOPE_OPEN.captures(line) {
                // A one-line `class Foo < Bar; end` opens nothing.
                if m.get(5).is_some_and(|t| t.as_str().contains("end")) {
                    continue;
                }
                let indent = m[1].len();
                let mut path = stack.last().map(|s| s.1.clone()).unwrap_or_default();
                let name = m[3].trim_start_matches("::");
                if m[3].starts_with("::") {
                    path.clear();
                }
                path.extend(name.split("::").map(String::from));
                let class = (&m[2] == "class").then(|| {
                    classes.push(Class {
                        path: path.clone(),
                        superclass: m.get(4).map_or(String::new(), |s| s.as_str().trim_start_matches("::").to_string()),
                        autocorrect: false,
                        excluded: false,
                    });
                    classes.len() - 1
                });
                stack.push((indent, path, class));
            } else if let Some(m) = SCOPE_END.captures(line) {
                if stack.last().is_some_and(|s| s.0 == m[1].len()) {
                    stack.pop();
                }
            } else if let Some(&(_, _, Some(i))) = stack.last() {
                let body = line.trim();
                classes[i].autocorrect |= body.starts_with("extend AutoCorrector");
                classes[i].excluded |= body.starts_with("exclude_from_registry");
            }
        }
    }
    // A cop descends from RuboCop::Cop::Base (or ::Cop), directly or through classes found here.
    // The superclass constant resolves lexically: the class's enclosing scopes, innermost first.
    let find = |at: usize| {
        let c = &classes[at];
        let name: Vec<String> = c.superclass.split("::").map(String::from).collect();
        (0..c.path.len()).rev().find_map(|n| {
            let mut full = c.path[..n].to_vec();
            full.extend(name.iter().cloned());
            classes.iter().position(|o| o.path == full).filter(|&j| j != at)
        })
    };
    let root =
        |s: &str| matches!(s, "Base" | "Cop" | "Cop::Base" | "Cop::Cop" | "RuboCop::Cop::Base" | "RuboCop::Cop::Cop");
    let lineage = |i: usize| -> Option<bool> {
        // Some(autocorrect) when `classes[i]` is a cop class.
        let (mut at, mut autocorrect) = (i, false);
        for _ in 0..32 {
            autocorrect |= classes[at].autocorrect;
            match find(at) {
                Some(j) => at = j,
                None => return root(&classes[at].superclass).then_some(autocorrect),
            }
        }
        None
    };
    let mut out: Vec<(String, bool)> = classes
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.excluded && c.path.len() >= 4 && c.path[0] == "RuboCop" && c.path[1] == "Cop")
        .filter_map(|(i, c)| lineage(i).map(|ac| (c.path[2..].join("/"), ac)))
        .collect();
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// `Array(value)` for YAML values.
fn seq(v: Option<Value>) -> Vec<Value> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Seq(items)) => Arc::unwrap_or_clone(items),
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
                derived.0.insert(0, (cop, Value::Map(m.into())));
            }
        }
    }
}

fn determine_inherit_mode(hash: &Map, key: &str, cli_preview: Option<bool>) -> Map {
    let mode = hash
        .map(key)
        .and_then(|m| m.map("inherit_mode"))
        .or_else(|| hash.map("inherit_mode"))
        .cloned()
        .unwrap_or_default();
    with_preview_exclude_merge(mode, hash, cli_preview)
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
            result.insert(key.clone(), Value::Map(merge(b, d, root_mode, unset_nil).into()));
        } else if should_union(derived, base, root_mode, key) {
            let mut union = seq(Some(bv.clone()));
            for v in seq(Some(dv.clone())) {
                if !union.contains(&v) {
                    union.push(v);
                }
            }
            result.insert(key.clone(), Value::Seq(union.into()));
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
        let Some(Value::Seq(items)) = Arc::make_mut(m).get_mut("Exclude") else { continue };
        for item in Arc::make_mut(items).iter_mut() {
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

/// Config#comparable_version: a dotted numeric version, trailing zero segments dropped so the
/// comparison is Gem::Version's (`1.5.0 == 1.5`, `1.65 > 1.7`).
// ponytail: prerelease versions (`1.2.a`) give None; NewCops versions are validated numeric and
// VersionAdded values in default.yml are numeric.
fn gem_version(s: &str) -> Option<Vec<u64>> {
    let mut out: Vec<u64> = s
        .split('.')
        .map(|p| p.parse().ok().filter(|_| p.bytes().all(|b| b.is_ascii_digit())))
        .collect::<Option<_>>()?;
    while out.len() > 1 && out.last() == Some(&0) {
        out.pop();
    }
    Some(out)
}

/// Ruby's String#to_f.
fn ruby_to_f(s: &str) -> f64 {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^\s*[-+]?\d+(?:\.\d+)?").unwrap());
    RE.find(s).and_then(|m| m.as_str().trim().parse().ok()).unwrap_or(0.0)
}

/// Bundler::LockfileParser specs as `Lockfile#gem_versions` returns them: every `    name (version)`
/// spec line (4-space indent) of a GEM/GIT/PATH section, platform suffix dropped, last one wins.
fn locked_gem_versions(text: &str) -> HashMap<String, String> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^ {4}([^ (]\S*) \(([^-)]*)(?:-[^)]*)?\)!?$").unwrap());
    text.lines()
        .filter_map(|l| RE.captures(l.trim_end_matches('\r')))
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect()
}

#[test]
fn lockfile_gem_versions() {
    let v = locked_gem_versions(
        "GEM\n  remote: x\n  specs:\n    railties (8.1.4)\n      actionpack (= 8.1.4)\n    nokogiri (1.18.0-x86_64-linux)\n\nDEPENDENCIES\n  rails (~> 8.1)\n",
    );
    assert_eq!(v.get("railties").map(String::as_str), Some("8.1.4"));
    assert_eq!(v.get("nokogiri").map(String::as_str), Some("1.18.0"));
    assert!(!v.contains_key("actionpack") && !v.contains_key("rails"));
}

#[test]
fn gem_versions_compare_like_rubygems() {
    assert!(gem_version("1.65") > gem_version("1.7"));
    assert_eq!(gem_version("1.5.0"), gem_version("1.5"));
    assert!(gem_version("1.5") < gem_version("1.5.1"));
    assert_eq!(gem_version("N/A"), None);
    assert_eq!(gem_version("<<next>>"), None);
    assert_eq!(gem_version("1.x"), None);
}

/// Dir.pwd (no symlink resolution, like Ruby).
pub fn cwd() -> PathBuf {
    // Ruby's Dir.pwd is getcwd(3), the physical path (/private/tmp, not /tmp); so is current_dir.
    // Read once: cops ask per file, getcwd is slow on macOS, and rbcop never changes directory.
    static CWD: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    CWD.get_or_init(|| std::env::current_dir().unwrap_or_default()).clone()
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
