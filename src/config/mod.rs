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
use value::{Map, Value, parse_yaml, parse_yaml_bytes, parse_yaml_file, ruby_regex};

/// RuboCop's own default.yml, vendored from the release rbcop targets (MIT, see config/LICENSE).
pub const DEFAULT_YML: &str = include_str!("../../config/default.yml");
/// Where `-d` says built-in config files come from (RuboCop names the gem's file on disk).
const BUILT_IN: &str = "<rbcop built-in>";
const BUILT_IN_DEFAULT: &str = "<rbcop built-in>/rubocop/config/default.yml";
pub const RUBOCOP_VERSION: &str = "1.91.0";
const DOTFILE: &str = ".rubocop.yml";

/// Command-line switches that change how configs resolve.
#[derive(Clone, Debug, Default)]
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
    /// `-d/--debug` (ConfigLoader.debug): config loading prints what it loads (Store::take_debug).
    pub debug: bool,
}

/// One include/exclude entry (FilePatterns / PathUtil.match_path?).
#[derive(Clone, Debug)]
pub enum Pattern {
    Exact(String),
    Glob(String),
    Regex(value::RubyRegex),
}

impl Pattern {
    pub fn from(v: &Value) -> Option<Pattern> {
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
    // Cheapest test first: every glob that misses lands here, and almost no file is hidden (#388).
    let p = Path::new(path);
    let hidden_file = path.contains("/.") && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
    let hidden_dir = || {
        p.parent().is_some_and(|d| {
            d.components().any(|c| matches!(c, Component::Normal(n) if n.to_string_lossy().starts_with('.')))
        })
    };
    hidden_file && glob::fnmatch(pattern, path, true) && !hidden_dir()
}

fn patterns(map: &Map, key: &str) -> Option<Vec<Pattern>> {
    match map.get(key)? {
        Value::Seq(items) => Some(items.iter().filter_map(Pattern::from).collect()),
        Value::Null => None,
        v => Some(Pattern::from(v).into_iter().collect()),
    }
}

/// An Include or Exclude list, shared by the cops of one config that have equal lists (`intern`).
type Patterns = Option<Arc<[Pattern]>>;

/// A cop's effective settings in one resolved config.
#[derive(Clone, Debug)]
pub struct CopConfig {
    pub enabled: bool,
    pub severity: Option<Severity>,
    include: Patterns,
    exclude: Patterns,
    /// Department params merged under the cop's (Config#for_badge).
    pub params: Map,
}

/// Gives the cops of one config with equal Include (or Exclude) lists one shared list, so that
/// `Resolved::relevant` matches each list once per file, not once per cop (#388).
fn intern(seen: &mut HashMap<String, Arc<[Pattern]>>, cop: &mut CopConfig) {
    for list in [&mut cop.include, &mut cop.exclude] {
        let Some(l) = list else { continue };
        let key: String = l
            .iter()
            .map(|p| match p {
                Pattern::Exact(s) => format!("e{s}\0"),
                Pattern::Glob(s) => format!("g{s}\0"),
                Pattern::Regex(r) => format!("r{}\0", r.as_str()),
            })
            .collect();
        *list = Some(seen.entry(key).or_insert_with(|| l.clone()).clone());
    }
}

/// `Resolved::relevant`'s answers for the file a thread is inspecting, by the lists' identity.
struct RelevantMemo {
    file: PathBuf,
    base: PathBuf,
    rel: String,
    abs: String,
    /// (include, exclude, relevant); holding the lists keeps their addresses from being reused.
    seen: Vec<(Patterns, Patterns, bool)>,
}

thread_local! {
    static RELEVANT: std::cell::RefCell<Option<RelevantMemo>> = const { std::cell::RefCell::new(None) };
}

fn same_list(a: &Patterns, b: &Patterns) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
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
    /// `cop_config[key]` as a number, a Float truncated (see Map::num: a String one raises).
    pub fn int(&self, key: &str) -> Option<i64> {
        self.params.num(key).map(|n| n.f64() as i64)
    }
    /// `cop_config[key]` as an Integer or a Float (Map::num).
    pub fn num(&self, key: &str) -> Option<value::Num> {
        self.params.num(key)
    }
    /// `Integer(cop_config.fetch(key, default))` (Map::integer_fetch).
    pub fn integer_fetch(&self, key: &str, default: i64) -> i64 {
        self.params.integer_fetch(key, default)
    }
    /// `Integer(cop_config[key] || default)` (Map::integer_or).
    pub fn integer_or(&self, key: &str, default: i64) -> i64 {
        self.params.integer_or(key, default)
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
    /// Each implemented cop's config, in the order of the cop names it was built from (`cop_at`).
    cop_list: Vec<(&'static str, CopConfig)>,
    /// Index in `cop_list` by name, with a cheap hash (the keys are cop names).
    cops: HashMap<&'static str, usize, std::hash::BuildHasherDefault<Fnv>>,
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
    /// Config#path_sourced_gems_in_target, read on first use.
    path_gems: std::sync::OnceLock<Vec<String>>,
    /// rubocop-rspec `RSpec: Language` (RuboCop::RSpec::Language.config), read on first use.
    pub rspec_language: std::sync::OnceLock<std::sync::Arc<crate::helpers::rspec::Language>>,
    /// The native plugins loaded (by any config) when this config's first file was inspected:
    /// Runner caches each config's team from then, so plugins loaded later add no cops to it.
    pub team_plugins: std::sync::OnceLock<Vec<&'static str>>,
    /// Config#loaded_path: the file this config was loaded from (None for the default).
    pub loaded_path: Option<PathBuf>,
}

impl Resolved {
    /// Whether a cop is in this config's team (Runner#mobilized_cop_classes): RuboCop's own always,
    /// a native plugin's when the plugin was loaded before the team was mobilized (see
    /// `team_plugins`; until then, when it is loaded at all).
    pub fn team_has(&self, cop: &str) -> bool {
        self.team_has_plugin(crate::helpers::cop_registry::plugin_of(cop))
    }

    /// `team_has` for a cop of `plugin` (None: RuboCop's own).
    pub fn team_has_plugin(&self, plugin: Option<&crate::helpers::cop_registry::NativePlugin>) -> bool {
        match plugin {
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
            cop_list: Vec::new(),
            cops: HashMap::default(),
            deprecated_warnings: Vec::new(),
            unregistered: Vec::new(),
            opts: opts.clone(),
            for_cop_cache: std::sync::OnceLock::new(),
            gem_versions: std::sync::OnceLock::new(),
            path_gems: std::sync::OnceLock::new(),
            rspec_language: std::sync::OnceLock::new(),
            team_plugins: std::sync::OnceLock::new(),
            loaded_path: loaded_path.map(Path::to_path_buf),
        };
        let mut lists = HashMap::new();
        for &name in cop_names {
            let mut cfg = r.build_cop(name, opts);
            intern(&mut lists, &mut cfg);
            r.set_cop(name, cfg);
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

    /// `Config.create(hash, loaded_path, check: false)`: this config's file with another hash.
    pub fn with_hash(&self, hash: Map, cop_names: &[&'static str]) -> Resolved {
        let mut r = Resolved::new(hash, self.loaded_path.as_deref(), &self.opts, cop_names);
        r.unregistered = self.unregistered.clone();
        if let Some(t) = self.team_plugins.get() {
            let _ = r.team_plugins.set(t.clone());
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

    /// Config#path_sourced_gems_in_target: the gems of the lockfile's PATH sections (`path:`
    /// dependencies and a `gemspec` Gemfile's own gem), when it reports any gems at all.
    pub fn path_sourced_gems_in_target(&self) -> &[String] {
        self.path_gems.get_or_init(|| {
            if self.gem_versions_in_target().is_none_or(|g| g.is_empty()) {
                return Vec::new();
            }
            let Some(file) = ["Gemfile.lock", "gems.locked"]
                .iter()
                .find_map(|n| self.base_dir.ancestors().map(|d| d.join(n)).find(|f| f.is_file()))
            else {
                return Vec::new();
            };
            let text = std::fs::read_to_string(file).unwrap_or_default();
            let mut out = Vec::new();
            let mut in_path = false;
            for line in text.lines().map(|l| l.trim_end_matches('\r')) {
                if !line.starts_with(' ') {
                    in_path = line == "PATH";
                } else if in_path && let Some(name) = line.strip_prefix("    ").filter(|r| !r.starts_with(' ')) {
                    out.extend(name.split(' ').next().map(str::to_string));
                }
            }
            out
        })
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

    /// Every input that resolves into this config's offenses, for the result cache key
    /// (Config#signature, plus what RuboCop reads outside the config: the target Ruby, the locked
    /// gem versions and the plugins in the team).
    pub fn signature(&self) -> String {
        let mut gems: Vec<_> = self.gem_versions_in_target().map(|g| g.iter().collect()).unwrap_or_default();
        gems.sort();
        format!(
            "{:?}|{:?}|{}|{:?}|{:?}|{:?}",
            self.hash,
            self.base_dir,
            self.target_ruby,
            self.opts,
            self.team_plugins.get(),
            gems
        )
    }

    pub fn all_cops(&self) -> &Map {
        static EMPTY: Map = Map(Vec::new());
        self.hash.map("AllCops").unwrap_or(&EMPTY)
    }

    pub fn cop(&self, name: &str) -> &CopConfig {
        &self.cop_list[self.cops[name]].1
    }

    /// The config of `name`, the `i`th of the cop names this config was built from (as
    /// `crate::cops::ALL` is in rbcop's run): no hashing for the runner's per-file scan of every
    /// cop (#388).
    pub fn cop_at(&self, i: usize, name: &str) -> &CopConfig {
        match self.cop_list.get(i) {
            Some((n, c)) if *n == name => c,
            _ => self.cop(name),
        }
    }

    fn set_cop(&mut self, name: &'static str, cfg: CopConfig) {
        match self.cops.get(name) {
            Some(&i) => self.cop_list[i].1 = cfg,
            None => {
                self.cops.insert(name, self.cop_list.len());
                self.cop_list.push((name, cfg));
            }
        }
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
        params.insert_all(self.hash.map(name).cloned().unwrap_or_default().0);
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
            Some(&i) => self.cop_list[i].1.enabled,
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
        params.insert_all(cop.0);
        if let Some(u) = union {
            params.insert("Exclude", u);
        }
        let severity = params.str("Severity").and_then(Severity::parse);
        CopConfig {
            enabled,
            severity,
            include: patterns(&params, "Include").map(Into::into),
            exclude: patterns(&params, "Exclude").map(Into::into),
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
        // Regexps charge a per-file step budget (value::charge), so their lists match every time.
        let memo =
            !cop.include.iter().chain(&cop.exclude).flat_map(|l| l.iter()).any(|p| matches!(p, Pattern::Regex(_)));
        RELEVANT.with(|m| {
            let mut m = m.borrow_mut();
            let current = m.as_ref().is_some_and(|m| {
                m.file.as_os_str() == file.as_os_str() && m.base.as_os_str() == self.base_dir.as_os_str()
            });
            if !current {
                *m = Some(RelevantMemo {
                    file: file.to_path_buf(),
                    base: self.base_dir.clone(),
                    rel: self.relative(file),
                    abs: file.display().to_string(),
                    seen: Vec::new(),
                });
            }
            let m = m.as_mut().unwrap();
            if memo
                && let Some(hit) =
                    m.seen.iter().find(|s| same_list(&s.0, &cop.include) && same_list(&s.1, &cop.exclude))
            {
                return hit.2;
            }
            let relevant = Self::relevant_to(cop, &m.rel, &m.abs);
            if memo {
                m.seen.push((cop.include.clone(), cop.exclude.clone(), relevant));
            }
            relevant
        })
    }

    fn relevant_to(cop: &CopConfig, rel: &str, abs: &str) -> bool {
        let included = match &cop.include {
            None => true,
            Some(pats) => {
                pats.iter().any(|p| p.matches(rel))
                    || (!rel.starts_with("..") && pats.iter().any(|p| p.is_absolute_or_parent() && p.matches(abs)))
                    || (rel.starts_with("..") && pats.iter().any(|p| p.matches(abs)))
            }
        };
        let excluded = cop.exclude.as_ref().is_some_and(|pats| pats.iter().any(|p| p.matches(rel) || p.matches(abs)));
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
    files: HashMap<PathBuf, (Map, validator::Output, String)>,
    /// Everything config loading has written to stderr so far.
    emitted: validator::Output,
    /// ConfigLoader.default_configuration has been loaded (under `-d`, announced).
    default_loaded: bool,
    gem_dirs: HashMap<String, PathBuf>,
    features: HashMap<String, Result<Feature, String>>,
    /// The `plugins:`/`require:` entry whose loading failed (integrate_plugins).
    failed_entry: Option<String>,
    loaded_plugins: Vec<String>,
    /// ConfigLoader.loaded_plugins proper: the `plugins:` entries loaded, in order (a
    /// plugin-capable `require:` loads as a plugin but is not added there).
    plugin_entries: Vec<String>,
    /// ConfigLoader.loaded_features: every `require:` entry.
    required: Vec<String>,
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
    /// Remote config files (their cache paths) with their URLs (RemoteConfig#uri).
    remote: HashMap<PathBuf, String>,
    /// ConfigLoader.cache_root, memoized at first use: the `configured` part of the root remote
    /// configs download into (`--cache-root` or a CacheRootDirectory; None for the default).
    remote_root: Option<Option<String>>,
    resolved: HashMap<Option<PathBuf>, Arc<Resolved>>,
    forced: Option<Arc<Resolved>>,
    pub warnings: Vec<String>,
}

/// What `-d` has config loading print (to stdout) since the last take_debug: the CLI prints it
/// where RuboCop's loading of that config would.
static DEBUG_OUT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

fn debug_out() -> std::sync::MutexGuard<'static, String> {
    DEBUG_OUT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn take_debug() -> String {
    std::mem::take(&mut *debug_out()).replace([ONCE, END_ONCE], "")
}

/// Around what `-d` prints once per run (the defaults and each plugin's config loading), which a
/// cached load_file does not replay.
const ONCE: char = '\u{1}';
const END_ONCE: char = '\u{2}';

/// `printed` without its ONCE parts.
fn replayable(printed: &str) -> String {
    let mut out = String::new();
    let mut rest = printed;
    while let Some(i) = rest.find(ONCE) {
        out.push_str(&rest[..i]);
        rest = rest[i..].find(END_ONCE).map_or("", |j| &rest[i + j + 1..]);
    }
    out.push_str(rest);
    out
}

/// ConfigFinder.project_root: the directory of the topmost Gemfile (else gems.rb) above `dir`.
pub fn project_root(dir: &Path) -> Option<PathBuf> {
    let file = find_last_upwards(&["Gemfile"], dir).or_else(|| find_last_upwards(&["gems.rb"], dir))?;
    file.parent().map(Path::to_path_buf)
}

impl Store {
    /// `cop_names` are the cops rbcop implements; their configs are precomputed per resolved config.
    pub fn new(opts: Options, cop_names: Vec<&'static str>) -> Result<Store, String> {
        let cwd = cwd();
        let default_text = DEFAULT_YML;
        let Value::Map(default) = parse_yaml(default_text)? else { return Err("bad default.yml".into()) };
        let mut default = Arc::unwrap_or_clone(default);
        make_excludes_absolute(&mut default, &cwd);
        let project_root = project_root(&cwd);
        let mut store = Store {
            opts,
            cwd,
            project_root,
            cop_names,
            default,
            files: HashMap::new(),
            emitted: Vec::new(),
            default_loaded: false,
            gem_dirs: HashMap::new(),
            features: HashMap::new(),
            failed_entry: None,
            loaded_plugins: Vec::new(),
            plugin_entries: Vec::new(),
            required: Vec::new(),
            custom_cops: false,
            copless_plugins: Vec::new(),
            plugin_constants: Default::default(),
            open_custom: false,
            open_constants: Default::default(),
            dir_to_file: HashMap::new(),
            loading: Vec::new(),
            remote: HashMap::new(),
            remote_root: None,
            resolved: HashMap::new(),
            forced: None,
            warnings: Vec::new(),
        };
        if store.opts.force_default_config {
            store.default_configuration();
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

    /// Version.parser_version's ConfigLoader.load_yaml_configuration of the config file found from
    /// the working directory (not `-c`), which `-d` names.
    pub fn debug_pwd_yaml(&mut self) {
        let shown = self.find_config_path(&self.cwd).map_or(BUILT_IN_DEFAULT.into(), |p| p.display().to_string());
        self.debug(&format!("configuration from {shown}\n"));
    }

    /// CacheConfig.root_dir_from_toplevel_config: `AllCops: CacheRootDirectory` as written in the
    /// config file found from the working directory (no inheritance, no -c).
    fn toplevel_cache_root(&self) -> Option<String> {
        let path = self.find_config_path(&self.cwd)?;
        let text = std::fs::read(&path).ok()?;
        if !text.windows(18).any(|w| w == b"CacheRootDirectory") {
            return None;
        }
        let text = match text.windows(2).any(|w| w == b"<%") {
            true => render_erb(&path, &text).ok().flatten().unwrap_or(text),
            false => text,
        };
        match parse_yaml_bytes(&text, &path.display().to_string()).ok()? {
            Value::Map(m) => m.map("AllCops")?.get("CacheRootDirectory")?.as_string(),
            _ => None,
        }
    }

    /// CLI#act_on_options' `ConfigLoader.cache_root(@options[:cache_root])`, before the pwd config
    /// loads: memoized by its first use, so a remote config in the `-c` file (loaded earlier, by
    /// ConfigStore#apply_options!) keeps the root without `--cache-root`.
    pub fn early_cache_root(&mut self, cache_root_override: Option<String>) -> Option<String> {
        if self.remote_root.is_none() {
            self.remote_root = Some(cache_root_override.or_else(|| self.toplevel_cache_root()));
        }
        self.remote_root.clone().flatten()
    }

    /// `ConfigLoader.cache_root = ResultCache.cache_root(...)`: the root later remote configs use.
    pub fn set_cache_root(&mut self, configured: Option<String>) {
        self.remote_root = Some(configured);
    }

    /// A config so far loaded `require:`s Ruby code rbcop cannot load (custom cops, or a plugin
    /// gem it could not integrate).
    /// ConfigLoader.loaded_plugins and loaded_features: every `plugins:`/`require:` name loaded so far.
    pub fn loaded_plugins(&self) -> &[String] {
        &self.loaded_plugins
    }

    /// RuboCop::Version.extension_versions: `  - <gem> <version>` for each loaded plugin, in load
    /// order, then for each other `require:`d `rubocop-*` feature, sorted. A native plugin is at its
    /// pinned version; another gem at its installed one, and left out when it is not installed.
    // ponytail: RuboCop reads a feature's version constant (RuboCop::<Name>::Version::STRING); the
    // gem's version stands in for it.
    pub fn extension_versions(&self) -> Vec<String> {
        let mut features: Vec<String> = self.required.clone();
        features.sort();
        features.dedup();
        features.retain(|f| f.contains("rubocop-") && !self.plugin_entries.contains(f));
        let names: Vec<String> = self.plugin_entries.iter().cloned().chain(features).collect();
        names
            .into_iter()
            .filter(|n| n != INTERNAL_AFFAIRS)
            .filter_map(|n| {
                let version = match crate::helpers::cop_registry::native_plugin(&n) {
                    Some(native) => native.version.to_string(),
                    None => self.gem_version(&n)?,
                };
                Some(format!("  - {n} {version}"))
            })
            .collect()
    }

    /// The installed version of a gem, if any.
    fn gem_version(&self, gem: &str) -> Option<String> {
        if !gem.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)) || gem.starts_with('-') {
            return None;
        }
        let out = std::process::Command::new("ruby")
            .args(["-e", "print Gem::Specification.find_by_name(ARGV[0]).version rescue exit 1", "--", gem])
            .current_dir(&self.cwd)
            .output()
            .ok()?;
        (out.status.success() && !out.stdout.is_empty()).then(|| String::from_utf8_lossy(&out.stdout).into_owned())
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
            let mut lists = HashMap::new();
            for &name in &self.cop_names {
                let mut cfg = r.build_cop(name, &self.opts);
                intern(&mut lists, &mut cfg);
                r.set_cop(name, cfg);
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

    pub fn color(&self) -> bool {
        self.opts.color
    }

    /// Under `-d`, `print` what RuboCop's config loading prints to stdout.
    fn debug(&mut self, s: &str) {
        if self.opts.debug {
            debug_out().push_str(s);
        }
    }

    /// ConfigLoader.default_configuration, which loads (and under `-d` names) default.yml the first
    /// time it is used. rbcop's default.yml is built in.
    fn default_configuration(&mut self) {
        if !self.default_loaded {
            self.default_loaded = true;
            self.debug(&format!("{ONCE}Default configuration from {BUILT_IN_DEFAULT}\n{END_ONCE}"));
        }
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
        self.debug(&format!("For {}: ", dir.display()));
        let hash = match &file {
            // ConfigLoader.apply_default_overrides: only the command line can turn preview on here.
            None => {
                self.default_configuration();
                apply_default_overrides(&self.default, &self.opts)
            }
            Some(path) => {
                let mut user = self.load_file(path, true)?;
                validator::check_target_ruby(&user, path, &self.cwd)?;
                if self.opts.ignore_parent_exclusion {
                    self.debug("Ignoring AllCops/Exclude from parent folders");
                } else {
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
        if let Some((m, out, printed)) = self.files.get(path).cloned() {
            self.emit(&out);
            debug_out().push_str(&printed);
            return Ok(m);
        }
        let start = self.emitted.len();
        let printed_from = debug_out().len();
        let text = std::fs::read(path).map_err(|e| match e.kind() {
            // ConfigLoader.read_file's ConfigNotFoundError.
            std::io::ErrorKind::NotFound => format!("Configuration file not found: {}", path.display()),
            _ => format!("{}: {e}", path.display()),
        })?;
        // ConfigLoader.load_yaml_configuration runs every config file through ERB; text without
        // `<%` comes out of ERB unchanged.
        let text = match text.windows(2).any(|w| w == b"<%") {
            true => match render_erb(path, &text)? {
                Some(yaml) => yaml,
                None => {
                    self.warnings.push(format!(
                        "{}: ERB in config needs ruby, which cannot be run; parsed as plain YAML",
                        path.display()
                    ));
                    text
                }
            },
            false => text,
        };
        let parsed = parse_yaml_bytes(&text, &path.display().to_string())?;
        // ConfigLoader.load_yaml_configuration
        self.debug(&format!("configuration from {}\n", path.display()));
        let mut hash = match parsed {
            Value::Map(m) => Arc::unwrap_or_clone(m),
            // `yaml_tree_to_hash(yaml_tree) || {}`
            Value::Null | Value::Bool(false) => Map::default(),
            // A ValidationError, which the CLI prints as `Error: ...`.
            _ => return Err(format!("Error: Malformed configuration in {}", path.display())),
        };
        // Plugins (and plugin-capable `require:` gems) only matter here for their default configs.
        let mut plugins: Vec<(String, Load)> = Vec::new();
        let mut migrate = Vec::new();
        let mut require_from = 0;
        for key in ["plugins", "require"] {
            if key == "require" {
                require_from = plugins.len();
            }
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
                            // `DEFAULT_PLUGIN_CONFIG.merge(plugin_config)`: a value that is no Hash
                            // raises TypeError, before any plugin loads (#619)
                            if key == "plugins" && config.as_map().is_none() {
                                let class = match config {
                                    Value::Null => "nil",
                                    Value::Bool(true) => "true",
                                    Value::Bool(false) => "false",
                                    Value::Int(_) => "Integer",
                                    Value::Float(_) => "Float",
                                    Value::Seq(_) => "Array",
                                    Value::Sym(_) => "Symbol",
                                    Value::Regex(_) => "Regexp",
                                    _ => "String",
                                };
                                let out: validator::Output = migrate.into_iter().map(|(_, b, s)| (b, s)).collect();
                                self.emit(&out);
                                return Err(format!("no implicit conversion of {class} into Hash"));
                            }
                            let config = config.as_map();
                            if config.and_then(|c| c.get("enabled")) == Some(&Value::Bool(false)) {
                                continue;
                            }
                            let named = config.and_then(|c| c.get("plugin_class_name")).and_then(Value::as_string);
                            // BUILTIN_INTERNAL_PLUGINS is looked up for a String entry only: a Hash
                            // one requires its `require_path` (default: the name)
                            if name == INTERNAL_AFFAIRS {
                                let require_path = config.is_some_and(|c| c.truthy("require_path"));
                                plugins.push((name.clone(), Load::BuiltinHash { require_path, class: named }));
                                continue;
                            }
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
            local.push((i, file));
        }
        // The first entry that fails to load ends the run, after the warnings of the entries
        // before it.
        let names: Vec<String> = plugins.iter().map(|(n, _)| n.clone()).collect();
        self.required.extend(names[require_from..].iter().cloned());
        let failed = match self.integrate_plugins(plugins, require_from, &mut local) {
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
        // file defines register once it loads (integrate_plugins registers the ones before a gem).
        for (_, file) in local {
            self.register_local(&file);
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
            // ConfigValidator#validate reads ConfigLoader.default_configuration.
            self.default_configuration();
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
        let printed = replayable(&debug_out()[printed_from..]);
        self.files.insert(path.to_path_buf(), (hash.clone(), out, printed));
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
        for (index, entry) in entries.into_iter().enumerate() {
            // ConfigLoaderResolver#inherited_file: a URL, an absolute path, a path relative to the
            // URL of a remote file, or a path relative to this file.
            let remote = if entry.starts_with("http://") || entry.starts_with("https://") {
                Some((entry.clone(), ""))
            } else if !Path::new(&entry).is_absolute() {
                self.remote.get(path).map(|url| (url.clone(), entry.as_str()))
            } else {
                None
            };
            let base_path = match remote {
                Some((url, relative)) => {
                    let Some((url, cache)) = self.remote_file(&url, relative)? else {
                        self.warnings.push(format!(
                            "{}: remote inherit_from {entry} needs ruby, which cannot be run; skipped",
                            path.display()
                        ));
                        continue;
                    };
                    // RemoteConfig#inherit_from_remote rewrites the path of the URI it shares
                    // with the remote file's own, so a later relative entry there is relative
                    // to this one's URL.
                    if !relative.is_empty() {
                        self.remote.insert(path.to_path_buf(), url.clone());
                    }
                    self.remote.insert(cache.clone(), url);
                    cache
                }
                None => {
                    self.debug("Inheriting ");
                    expand_path(Path::new(&entry), dir)
                }
            };
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
            bases.push((index, base_path, base?));
        }
        // ConfigLoaderResolver#warn_on_duplicate_setting's `inherited_file`: the entry as written.
        let written = seq(hash.get("inherit_from").cloned());
        let file_is_remote = self.remote.contains_key(path);
        for (index, base_path, base) in bases.into_iter().rev() {
            override_department_setting_for_cops(&base, hash);
            override_enabled_for_disabled_departments(&base, hash);
            for (k, v) in base.0 {
                let Value::Map(v) = v else { continue };
                let only_base_has_include = v.contains("Include") && !hash.map(&k).is_some_and(|m| m.truthy("Include"));
                let merged = match hash.map(&k) {
                    Some(derived) => {
                        let mode = determine_inherit_mode(hash, &k, self.opts.preview);
                        if self.opts.debug && !file_is_remote {
                            let inherited = written.get(index).and_then(Value::as_string);
                            let mut out = Vec::new();
                            self.duplicate_settings(&v, derived, &mode, inherited.as_deref(), &mut out);
                            for key in out {
                                let shown = validator::smart_path(path, &self.cwd);
                                let inherited = inherited.as_deref().unwrap_or_default();
                                self.debug(&format!(
                                    "{shown}: {k}:{key} overrides the same parameter in {inherited}\n"
                                ));
                            }
                        }
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

    /// The keys `merge` (ConfigLoaderResolver#merge with `debug:`) takes from `derived` over a
    /// different `base` value that is neither merged nor unioned: warn_on_duplicate_setting.
    fn duplicate_settings(
        &self,
        base: &Map,
        derived: &Map,
        mode: &Map,
        inherited: Option<&str>,
        out: &mut Vec<String>,
    ) {
        for (key, dv) in &derived.0 {
            let Some(bv) = base.get(key) else { continue };
            if let (Value::Map(b), Value::Map(d)) = (bv, dv) {
                self.duplicate_settings(b, d, mode, inherited, out);
                continue;
            }
            if should_union(derived, base, mode, key) {
                continue;
            }
            // duplicate_setting?: not for a parent directory's file, a remote one or one inside an
            // installed gem (Gem.path; here, the gem directories looked up so far).
            let Some(file) = inherited else { continue };
            let in_gem = self
                .gem_dirs
                .values()
                .filter_map(|d| d.parent()?.parent())
                .any(|g| file.starts_with(&*g.to_string_lossy()));
            if file.starts_with("..")
                || bv == dv
                || file.starts_with("http://")
                || file.starts_with("https://")
                || in_gem
            {
                continue;
            }
            let listed = |m: &str| {
                mode.get(m).map(|v| seq(Some(v.clone())).iter().any(|x| x.as_string().as_deref() == Some(key)))
            };
            if matches!(bv, Value::Seq(_)) && listed("merge").or_else(|| listed("override")).unwrap_or(false) {
                continue;
            }
            out.push(key.clone());
        }
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
        self.debug("AllCops/Exclude ");
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
    // rubocop-* plugin checked so far uses. Local `require: ./file` entries (custom cops, `local`
    // with their places in `names`) register here when they come before a gem (#536, #579); the
    // caller registers the ones after the last. `names` from `require_from` on are `require:` entries.
    fn integrate_plugins(
        &mut self,
        names: Vec<(String, Load)>,
        require_from: usize,
        local: &mut Vec<(usize, PathBuf)>,
    ) -> Result<(), String> {
        let names: Vec<(usize, String, Load)> = names
            .into_iter()
            .enumerate()
            .filter(|(_, (n, _))| !n.starts_with('.') && !n.starts_with('/') && !self.loaded_plugins.contains(n))
            .map(|(i, (n, l))| (i, n, l))
            .collect();
        if names.is_empty() {
            return Ok(());
        }
        // Plugin.integrate_plugins (not a plain `require:`), once the gems have loaded.
        let integrates = names.iter().any(|(_, _, l)| *l != Load::Require);
        // Plugin::Loader.load requires every plugin (registering its cops) before
        // ConfigurationIntegrator loads ConfigLoader.default_configuration, whose qualified keys
        // fill Registry#unqualified_cop_names, and then each plugin's config.
        // ponytail: a config's `plugins:` and plugin-capable `require:` gems merge their configs as
        // one batch; RuboCop integrates each such `require:` on its own, after `plugins:`.
        let mut configs: Vec<Result<&'static crate::helpers::cop_registry::NativePlugin, PathBuf>> = Vec::new();
        for (at, name, load) in names {
            // `require:` loads in order, each entry on its own: the `plugins:` batch and every gem
            // with a config before this entry have memoized Registry#unqualified_cop_names, and a
            // local file before it registers its cops now.
            if at >= require_from && !configs.is_empty() {
                crate::helpers::cop_registry::fill_unqualified_cop_names();
            }
            if at >= require_from || configs.is_empty() {
                for (_, file) in local.extract_if(.., |(i, _)| *i < at) {
                    self.register_local(&file);
                }
            }
            self.loaded_plugins.push(name.clone());
            if at < require_from {
                self.plugin_entries.push(name.clone());
            }
            let load = match load {
                Load::BuiltinHash { require_path: false, .. } => {
                    self.failed_entry = Some(name.clone());
                    return Err(format!("cannot load such file -- {name}"));
                }
                Load::BuiltinHash { class: None, .. } => {
                    self.failed_entry = Some(name.clone());
                    return Err(plugin_class_unknown(&name));
                }
                Load::BuiltinHash { class: Some(c), .. } => Load::NamedPlugin(c),
                l => l,
            };
            if let Some(native) = crate::helpers::cop_registry::native_plugin(&name) {
                // A plugin rbcop implements: its cops register and its vendored default.yml merges,
                // so the gem need not be installed. A `plugin_class_name` is looked up as for any
                // gem (#556, #568).
                // ponytail: with the gem not installed the name is not checked.
                if let Load::NamedPlugin(class) = &load
                    && let Ok(Feature { dir: Some(dir), .. }) = self.feature(&name)
                    && let Some(problem) =
                        named_plugin_problem(&name, class, &self.constant_libs(&name, dir.join("lib")))
                {
                    self.failed_entry = Some(name.clone());
                    return Err(problem);
                }
                crate::helpers::cop_registry::register_plugin(native);
                // The rules are the plugin class's: one naming another native plugin's class
                // integrates that plugin's default.yml (again), and this gem's cops run
                // unconfigured (#627).
                let rules = match &load {
                    Load::NamedPlugin(class) => crate::helpers::cop_registry::NATIVE_PLUGINS
                        .iter()
                        .find(|p| p.plugin_class == class.trim_start_matches("::"))
                        .unwrap_or(native),
                    _ => native,
                };
                configs.push(Ok(rules));
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
                return Err(plugin_class_unknown(&name));
            }
            // A Ruby library outside any gem (`require: set`): no cops.
            let Some(dir) = feature.dir else { continue };
            let lib = dir.join(lib);
            // Kernel.const_get of the name, then `.new(plugin_config)` on it.
            if let Load::NamedPlugin(class) = &load
                && let Some(problem) = named_plugin_problem(&name, class, &self.constant_libs(&name, lib.clone()))
            {
                self.failed_entry = Some(name.clone());
                return Err(problem);
            }
            let path = dir.join(config);
            // A plugin loads its gem (its cops, its config); a plain `require` loads the files
            // it reaches, and the gem's config files only when that code injects them into the
            // default configuration (an extension's Inject.defaults!).
            let (files, paths) = if load == Load::Require {
                let rubocop = self.gem_dir("rubocop").ok();
                let files = match &feature.file {
                    Some(file) => required_files(&lib, file, rubocop.as_deref()),
                    None => ruby_files(&lib),
                };
                let paths: Vec<PathBuf> =
                    injected_configs(&files).into_iter().map(|p| dir.join(p)).filter(|p| p.is_file()).collect();
                (files, paths)
            } else {
                (ruby_files(&lib), [path.clone()].into_iter().filter(|p| p.is_file()).collect())
            };
            let merge = !paths.is_empty();
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
            let mut config = Map::default();
            for path in &paths {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
                if let Value::Map(m) = parse_yaml_file(&text, &path.display().to_string())? {
                    for (k, v) in Arc::unwrap_or_clone(m).0 {
                        config.insert(k, v);
                    }
                }
            }
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
            configs.extend(paths.into_iter().map(Err));
        }
        // (A gem with a config is a plugin, or an older extension whose inject_defaults! loads
        // the default configuration as well.)
        if !configs.is_empty() {
            crate::helpers::cop_registry::fill_unqualified_cop_names();
        }
        if integrates {
            self.default_configuration();
        }
        let mut combined = self.default.clone();
        let mut configured: Vec<String> = Vec::new();
        for config in configs {
            // ponytail: an extension's injected config (a plain `require:`) is named as a plugin's.
            self.debug(&format!("{ONCE}Plugin "));
            let mut plugin = match config {
                Ok(native) => {
                    self.debug(&format!("configuration from {BUILT_IN}/{}/config/default.yml\n", native.gem));
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
            self.debug(&END_ONCE.to_string());
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

    /// A `require:`d local file's cop classes join the registry. A cop is a class descending from
    /// a cop base, not excluded from the registry (ruby_cops); only a file in which none is found
    /// leaves the registry open.
    // ponytail: files the local file itself requires are not read.
    fn register_local(&mut self, file: &Path) {
        let cops = ruby_cops_in(vec![file.to_path_buf()]);
        if cops.is_empty() {
            self.open_custom = true;
            ruby_file_constants(file, &mut self.open_constants);
        }
        crate::helpers::cop_registry::register_custom(cops);
    }

    /// ConfigLoaderResolver#merge_with_default (§4) against the current defaults.
    fn merge_with_default(&mut self, config: Map) -> Map {
        self.default_configuration();
        // It loads ConfigLoader.default_configuration, whose `AllCops` key memoizes
        // Registry#unqualified_cop_names (before any subdirectory config loads its plugins).
        crate::helpers::cop_registry::fill_unqualified_cop_names();
        merge_with_default(&self.default, config, true, &self.opts)
    }

    /// Gem dir for inherit_gem and plugins, as `Gem::Specification.find_by_name(name).gem_dir` reports it.
    /// The lib/ directories whose classes and modules Kernel.const_get sees once the gem `name`
    /// (lib/ `own`) is required: its own, rubocop's, lint_roller's and those of the plugins and
    /// features loaded before it (#590).
    fn constant_libs(&mut self, name: &str, own: PathBuf) -> Vec<PathBuf> {
        let mut libs = vec![own];
        for gem in ["rubocop", "lint_roller"] {
            if let Ok(d) = self.gem_dir(gem) {
                libs.push(d.join("lib"));
            }
        }
        let earlier: Vec<String> = self.loaded_plugins.iter().filter(|n| *n != name).cloned().collect();
        for n in earlier {
            if let Ok(Feature { dir: Some(d), .. }) = self.feature(&n) {
                libs.push(d.join("lib"));
            }
        }
        libs
    }

    /// RemoteConfig#file (downloading into ConfigLoader.cache_root, refreshed after a day), for
    /// `url` or for `relative` resolved against it: the effective URL and the cache file. Run by
    /// the project's ruby with RuboCop's own code (src/config/remote_config.rb); None when ruby
    /// cannot be run. On failure RuboCop prints the message and its backtrace; rbcop the message.
    fn remote_file(&mut self, url: &str, relative: &str) -> Result<Option<(String, PathBuf)>, String> {
        let configured = self.early_cache_root(None).unwrap_or_default();
        let Ok(out) = std::process::Command::new("ruby")
            .args(["-e", include_str!("remote_config.rb"), url, relative, &configured])
            .current_dir(&self.cwd)
            .output()
        else {
            return Ok(None);
        };
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim_end().to_string());
        }
        let out = String::from_utf8_lossy(&out.stdout);
        let (url, cache) = out.split_once('\0').ok_or_else(|| format!("{url}: ruby: unexpected output"))?;
        Ok(Some((url.to_string(), PathBuf::from(cache))))
    }

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

/// `Dir.chdir(File.dirname(path)) { ERB.new(contents).result }`, run by the project's Ruby (the
/// one inherit_gem uses). On failure RuboCop's CLI prints the exception's message and backtrace;
/// rbcop prints the message and the template's own frames (`(erb):N`), not RuboCop's. None when
/// ruby cannot be run.
// ponytail: plain ruby, not RuboCop's process, so the template does not see RuboCop's constants.
fn render_erb(path: &Path, text: &[u8]) -> Result<Option<Vec<u8>>, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // No local variables before `result`: ERB's default binding is a fresh top level. Inside
    // RuboCop the rubocop gem is activated; templates such as rubocop-shopify's read its version
    // from Gem.loaded_specs, so that sees the version rbcop matches.
    let script = format!(
        r#"begin
  Gem.loaded_specs["rubocop"] = Gem::Specification.new("rubocop", "{RUBOCOP_VERSION}")
  $stdout.binmode
  print ERB.new($stdin.binmode.read.force_encoding(Encoding::UTF_8)).result
rescue StandardError, SyntaxError, LoadError => e
  $stderr.puts e.message, e.backtrace.grep(/\A\(erb\)/)
  exit 1
end"#
    );
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let Ok(mut child) = Command::new("ruby")
        .args(["-rerb", "-e", &script])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return Ok(None);
    };
    // A template that exits before reading its input closes the pipe; the exit status tells.
    let _ = child.stdin.take().expect("piped stdin").write_all(text);
    let out = child.wait_with_output().map_err(|e| format!("{}: ruby: {e}", path.display()))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim_end().to_string());
    }
    Ok(Some(out.stdout))
}

/// Plugin::LoadError when no plugin class can be determined for an entry.
fn plugin_class_unknown(name: &str) -> String {
    format!(
        "Error: Failed to load plugin `{name}` because the corresponding plugin class could not be determined \
         for instantiation.\nTry upgrading it first (e.g., `bundle update {name}`).\nIf `{name}` is not yet a \
         plugin, use `require: {name}` instead of `plugins: {name}` in your configuration.\n\nFor further \
         assistance, check with the developer regarding the following points:\n\
         https://docs.rubocop.org/rubocop/plugin_migration_guide.html"
    )
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
    /// A Hash entry naming RuboCop's built-in plugin: no built-in there. Without a `require_path`
    /// the name is required (no such file); with one (`rubocop/cop/internal_affairs`) the plugin
    /// class comes from a `plugin_class_name` only.
    BuiltinHash { require_path: bool, class: Option<String> },
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

/// The config files (relative to the gem directory) that loading these files merges into
/// ConfigLoader's default configuration, in call order: an extension's `Inject.<method>` called
/// at the top level (rubocop-rails' old `Inject.defaults!`, rubocop-github's
/// `Inject.default_defaults!` / `rails_defaults!`), where the files also set the default
/// configuration. The file is the one the constant the method (or a `self.` method it calls)
/// reads names (`CONFIG_DEFAULT = PROJECT_ROOT.join("config", "default_cops.yml")`);
/// config/default.yml for a `defaults!` / `inject_defaults!` whose file is not found that way.
/// (Code that sets it only from a method RuboCop calls later, as standard's plugin, does not.)
// ponytail: the call is found outside any `def`, the assignment by name, and a constant by its
// last name; a path built other than from string literals is not followed.
fn injected_configs(files: &[PathBuf]) -> Vec<String> {
    use ruby_prism::Visit;
    #[derive(Default)]
    struct Scan {
        /// `def self.m`: the constants and the receiverless or `self.` calls in its body
        defs: HashMap<String, (Vec<String>, Vec<String>)>,
        /// `NAME = ...`: the path its string literals spell, when it is a .yml file
        consts: HashMap<String, String>,
        /// top-level `Const.m` calls
        calls: Vec<String>,
        in_def: Option<String>,
    }
    impl<'pr> Visit<'pr> for Scan {
        fn visit_def_node(&mut self, n: &ruby_prism::DefNode<'pr>) {
            if self.in_def.is_some() || n.receiver().is_none_or(|r| r.as_self_node().is_none()) {
                return;
            }
            let name = String::from_utf8_lossy(n.name().as_slice()).into_owned();
            self.defs.entry(name.clone()).or_default();
            self.in_def = Some(name);
            if let Some(b) = n.body() {
                self.visit(&b);
            }
            self.in_def = None;
        }
        fn visit_constant_read_node(&mut self, n: &ruby_prism::ConstantReadNode<'pr>) {
            if let Some(d) = &self.in_def {
                let name = String::from_utf8_lossy(n.name().as_slice()).into_owned();
                self.defs.get_mut(d).unwrap().0.push(name);
            }
        }
        fn visit_constant_write_node(&mut self, n: &ruby_prism::ConstantWriteNode<'pr>) {
            struct Strs(Vec<String>);
            impl<'pr> Visit<'pr> for Strs {
                fn visit_string_node(&mut self, n: &ruby_prism::StringNode<'pr>) {
                    self.0.push(String::from_utf8_lossy(n.unescaped()).into_owned());
                }
            }
            let mut strs = Strs(Vec::new());
            strs.visit(&n.value());
            let parts: Vec<String> = strs.0.into_iter().filter(|s| s != "." && s != "..").collect();
            if parts.last().is_some_and(|s| s.ends_with(".yml")) {
                self.consts.insert(String::from_utf8_lossy(n.name().as_slice()).into_owned(), parts.join("/"));
            }
            ruby_prism::visit_constant_write_node(self, n);
        }
        fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
            let name = String::from_utf8_lossy(n.name().as_slice()).into_owned();
            match &self.in_def {
                Some(d) if n.receiver().is_none_or(|r| r.as_self_node().is_some()) => {
                    self.defs.get_mut(d).unwrap().1.push(name)
                }
                None if n
                    .receiver()
                    .is_some_and(|r| r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some()) =>
                {
                    self.calls.push(name)
                }
                _ => {}
            }
            ruby_prism::visit_call_node(self, n);
        }
    }
    static SET: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"@default_configuration\b|\bdefault_configuration\s*=[^=]").unwrap());
    let texts: Vec<String> = files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
    if !texts.iter().any(|t| SET.is_match(t)) {
        return Vec::new();
    }
    let mut scan = Scan::default();
    for t in &texts {
        scan.visit(&ruby_prism::parse(t.as_bytes()).node());
    }
    // the config files a self method reads, through the self methods it calls
    fn reads(scan: &Scan, m: &str, seen: &mut Vec<String>) -> Vec<String> {
        if seen.iter().any(|s| s == m) {
            return Vec::new();
        }
        seen.push(m.to_string());
        let Some((consts, calls)) = scan.defs.get(m) else { return Vec::new() };
        let own: Vec<String> = consts.iter().filter_map(|c| scan.consts.get(c).cloned()).collect();
        if !own.is_empty() {
            return own;
        }
        calls.iter().flat_map(|c| reads(scan, c, seen)).collect()
    }
    let mut out: Vec<String> = Vec::new();
    for call in &scan.calls {
        let mut paths = reads(&scan, call, &mut Vec::new());
        if paths.is_empty() && matches!(call.as_str(), "defaults!" | "inject_defaults!") {
            paths.push("config/default.yml".to_string());
        }
        for p in paths {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
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
/// Plugin::Loader.constantize of a `plugin_class_name`: Kernel.const_get, then `.new` on it. The
/// error that ends the run when the name is no class.
// ponytail: an earlier plugin's plugin class is taken as this gem's; RuboCop integrates that
// plugin's rules instead and leaves this gem's cops unconfigured (docs/known-gaps.md).
fn named_plugin_problem(name: &str, class: &str, libs: &[PathBuf]) -> Option<String> {
    let bare = class.trim_start_matches("::");
    // `LintRoller::Rules.new(plugin_config)`: a keyword_init Struct (no `class` line)
    if bare == "LintRoller::Rules" {
        return Some("unknown keywords: enabled, require_path, plugin_class_name".into());
    }
    let files: Vec<PathBuf> = libs.iter().flat_map(|l| ruby_files(l)).collect();
    match ruby_constant_kinds(&files).get(bare) {
        // Plugin::ConfigurationIntegrator.validate_plugins! and LintRoller::Plugin#rules
        Some((true, _)) if bare == "LintRoller::Plugin" => {
            Some("Please implement `rules(context)` and return an instance of LintRoller::Rules".into())
        }
        Some((true, sup)) if sup.as_deref().is_some_and(|s| s.trim_start_matches("::") == "LintRoller::Plugin") => None,
        Some((true, _)) => Some(format!("undefined method 'supported?' for an instance of {bare}")),
        Some((false, _)) => Some(format!("undefined method 'new' for module {class}")),
        None => Some(format!("Failed while configuring plugin `{name}': no constant with name `{class}' was found.")),
    }
}

fn ruby_constant_kinds(files: &[PathBuf]) -> HashMap<String, (bool, Option<String>)> {
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
                let entry = out.entry(path.join("::")).or_insert((false, None));
                entry.0 |= &m[2] == "class";
                if let Some(sup) = m.get(4) {
                    entry.1.get_or_insert_with(|| sup.as_str().to_string());
                }
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
                // `extend AutoCorrector`, also qualified (`extend ::RuboCop::Cop::AutoCorrector`)
                // or parenthesized (`extend(AutoCorrector)`)
                static EXTEND: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
                    Regex::new(r"^extend(?:\s+|\s*\(\s*)(?:::)?(?:RuboCop::Cop::)?AutoCorrector\b").unwrap()
                });
                classes[i].autocorrect |= EXTEND.is_match(body);
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
    // `base.merge(derived)` entry by entry; a removed key is None until the end. Keys are looked
    // up through an index, so a mapping with many keys merges in linear time.
    let mut result: Vec<(String, Option<Value>)> = base.0.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect();
    let index: HashMap<&str, usize> = base.0.iter().enumerate().map(|(i, (k, _))| (k.as_str(), i)).collect();
    for (key, dv) in &derived.0 {
        let Some(&i) = index.get(key.as_str()) else {
            result.push((key.clone(), Some(dv.clone())));
            continue;
        };
        let bv = &base.0[i].1;
        result[i].1 = if unset_nil && *dv == Value::Null {
            None
        } else if let (Value::Map(b), Value::Map(d)) = (bv, dv) {
            Some(Value::Map(merge(b, d, root_mode, unset_nil).into()))
        } else if should_union(derived, base, root_mode, key) {
            let mut union = seq(Some(bv.clone()));
            for v in seq(Some(dv.clone())) {
                if !union.contains(&v) {
                    union.push(v);
                }
            }
            Some(Value::Seq(union.into()))
        } else {
            Some(dv.clone())
        };
    }
    Map(result.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect())
}

/// Config#make_excludes_absolute: every `Exclude` string becomes absolute against `base`. `map!`
/// changes the Array itself, so a value aliased to it (`Foo: *excludes`) changes too.
fn make_excludes_absolute(hash: &mut Map, base: &Path) {
    let mut memo = HashMap::new();
    for (_, v) in &hash.0 {
        let Some(Value::Seq(items)) = v.as_map().and_then(|m| m.get("Exclude")) else { continue };
        let absolute = items.iter().map(|item| match item {
            Value::Str(s) if !s.starts_with('/') => Value::Str(normalize(&base.join(s)).display().to_string()),
            item => item.clone(),
        });
        memo.insert(Arc::as_ptr(items) as usize, Some(Value::Seq(Arc::new(absolute.collect()))));
    }
    if memo.is_empty() {
        return;
    }
    for (_, v) in hash.0.iter_mut() {
        if let Some(new) = replace_shared(v, &mut memo) {
            *v = new;
        }
    }
}

/// `v` with the collections in `memo` (by address) replaced, or None if it holds none of them.
/// Each collection is rebuilt once, so values shared before stay shared (and an alias bomb is
/// walked once per collection).
fn replace_shared(v: &Value, memo: &mut HashMap<usize, Option<Value>>) -> Option<Value> {
    let addr = match v {
        Value::Seq(a) => Arc::as_ptr(a) as usize,
        Value::Map(m) => Arc::as_ptr(m) as usize,
        _ => return None,
    };
    if let Some(done) = memo.get(&addr) {
        return done.clone();
    }
    memo.insert(addr, None);
    let new = match v {
        Value::Seq(items) => {
            let changed: Vec<_> = items.iter().map(|i| replace_shared(i, memo)).collect();
            changed.iter().any(Option::is_some).then(|| {
                let items = items.iter().zip(changed).map(|(old, new)| new.unwrap_or_else(|| old.clone()));
                Value::Seq(Arc::new(items.collect()))
            })
        }
        Value::Map(m) => {
            let changed: Vec<_> = m.0.iter().map(|(_, v)| replace_shared(v, memo)).collect();
            changed.iter().any(Option::is_some).then(|| {
                let pairs =
                    m.0.iter().zip(changed).map(|((k, old), new)| (k.clone(), new.unwrap_or_else(|| old.clone())));
                Value::Map(Arc::new(Map(pairs.collect())))
            })
        }
        _ => None,
    };
    memo.insert(addr, new.clone());
    new
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

#[cfg(test)]
mod relevant_tests {
    use super::*;

    // Cops with equal lists share one (`intern`), and `relevant`'s per-file memo answers each cop
    // by its own lists as the file changes; a list with a regexp is matched every time (#388).
    #[test]
    fn shared_lists_and_the_per_file_memo() {
        let yaml = "Lint/Debugger:\n  Include: ['**/*_spec.rb']\n\
                    Style/Not:\n  Include: ['**/*_spec.rb']\n  Exclude: ['vendor/**/*']\n\
                    Layout/Other:\n  Include: ['**/*_spec.rb']\n  Exclude: ['vendor/**/*']\n\
                    Lint/Rx:\n  Exclude: !ruby/regexp /vendor/\n";
        let Ok(Value::Map(m)) = parse_yaml(yaml) else { panic!("yaml") };
        let names = ["Lint/Debugger", "Style/Not", "Layout/Other", "Lint/Rx"];
        let r = Resolved::new(Arc::unwrap_or_clone(m), None, &Options::default(), &names);
        let (debugger, not, other) = (r.cop("Lint/Debugger"), r.cop("Style/Not"), r.cop("Layout/Other"));
        assert!(same_list(&debugger.include, &not.include) && same_list(&not.exclude, &other.exclude));
        assert!(std::ptr::eq(r.cop_at(1, "Style/Not"), not) && std::ptr::eq(r.cop_at(0, "Style/Not"), not));
        let at = |p: &str| r.base_dir.join(p);
        for _ in 0..2 {
            for (file, want) in [
                ("a_spec.rb", [true, true, true, true]),
                ("vendor/a_spec.rb", [true, false, false, false]),
                ("a.rb", [false, false, false, true]),
                // A hidden file matches `**/*` outside hidden directories (match_path?).
                ("lib/.a_spec.rb", [true, true, true, true]),
                (".git/lib/.a_spec.rb", [false, false, false, true]),
            ] {
                let got = names.map(|n| r.relevant(r.cop(n), &at(file)));
                assert_eq!(got, want, "{file}");
            }
        }
    }
}
