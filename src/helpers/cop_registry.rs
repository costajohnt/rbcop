//! RuboCop's `Registry.global` (every cop in default.yml, not just the ported ones, plus the cops
//! of the native plugins loaded so far) and `ConfigObsoletion.legacy_cop_names`.

use regex::Regex;
use std::sync::{LazyLock, RwLock};

/// A plugin gem whose cops rbcop implements. Its `config/default.yml` is vendored, so the gem need
/// not be installed: `plugins:` / `require:` of it registers its cops and merges that file.
pub struct NativePlugin {
    pub gem: &'static str,
    /// The version in docs/plugins.md (what `-V` lists).
    pub version: &'static str,
    /// Its `default_lint_roller_plugin` class.
    pub plugin_class: &'static str,
    /// The departments its cops live in.
    pub departments: &'static [&'static str],
    /// The gem's config/default.yml at the version in docs/plugins.md.
    pub default_yml: &'static str,
    /// The gem's config/obsoletion.yml (ConfigObsoletion.files), if it ships one.
    pub obsoletion: Option<&'static str>,
}

pub const NATIVE_PLUGINS: &[NativePlugin] = &[
    NativePlugin {
        gem: "rubocop-performance",
        version: "1.27.0",
        plugin_class: "RuboCop::Performance::Plugin",
        departments: &["Performance"],
        default_yml: include_str!("../../config/plugins/rubocop-performance.yml"),
        obsoletion: None,
    },
    NativePlugin {
        gem: "rubocop-rails",
        version: "2.38.0",
        plugin_class: "RuboCop::Rails::Plugin",
        departments: &["Rails"],
        default_yml: include_str!("../../config/plugins/rubocop-rails.yml"),
        obsoletion: Some(include_str!("../../config/plugins/rubocop-rails-obsoletion.yml")),
    },
    NativePlugin {
        gem: "rubocop-minitest",
        version: "0.41.0",
        plugin_class: "RuboCop::Minitest::Plugin",
        departments: &["Minitest"],
        default_yml: include_str!("../../config/plugins/rubocop-minitest.yml"),
        obsoletion: None,
    },
    NativePlugin {
        gem: "rubocop-rspec",
        version: "3.10.2",
        plugin_class: "RuboCop::RSpec::Plugin",
        departments: &["RSpec"],
        default_yml: include_str!("../../config/plugins/rubocop-rspec.yml"),
        obsoletion: Some(include_str!("../../config/plugins/rubocop-rspec-obsoletion.yml")),
    },
    NativePlugin {
        gem: "rubocop-rspec_rails",
        version: "2.33.0",
        plugin_class: "RuboCop::RSpecRails::Plugin",
        departments: &["RSpecRails"],
        default_yml: include_str!("../../config/plugins/rubocop-rspec_rails.yml"),
        obsoletion: None,
    },
    NativePlugin {
        gem: "rubocop-factory_bot",
        version: "2.28.0",
        plugin_class: "RuboCop::FactoryBot::Plugin",
        departments: &["FactoryBot"],
        default_yml: include_str!("../../config/plugins/rubocop-factory_bot.yml"),
        obsoletion: None,
    },
    NativePlugin {
        gem: "rubocop-capybara",
        version: "3.1.0",
        plugin_class: "RuboCop::Capybara::Plugin",
        departments: &["Capybara", "Capybara/RSpec"],
        default_yml: include_str!("../../config/plugins/rubocop-capybara.yml"),
        obsoletion: None,
    },
];

pub fn native_plugin(gem: &str) -> Option<&'static NativePlugin> {
    NATIVE_PLUGINS.iter().find(|p| p.gem == gem)
}

/// The native plugin a cop belongs to, if any.
pub fn plugin_of(cop: &str) -> Option<&'static NativePlugin> {
    let dept = department_of(cop);
    NATIVE_PLUGINS.iter().find(|p| p.departments.contains(&dept))
}

/// Cop names in a default.yml, in file order.
fn cop_names(yml: &'static str) -> Vec<&'static str> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^([A-Z]\w*(?:/\w+)+):").unwrap());
    without_departments(RE.captures_iter(yml).map(|c| c.get(1).unwrap().as_str()).collect())
}

/// Drops the keys of nested departments (rubocop-capybara's `Capybara/RSpec:` configures the
/// department of `Capybara/RSpec/*`): the registry holds cops only.
fn without_departments<S: AsRef<str>>(names: Vec<S>) -> Vec<S> {
    let is_dept = |n: &str| names.iter().any(|o| o.as_ref().strip_prefix(n).is_some_and(|r| r.starts_with('/')));
    let keep: Vec<bool> = names.iter().map(|n| !is_dept(n.as_ref())).collect();
    names.into_iter().zip(keep).filter_map(|(n, k)| k.then_some(n)).collect()
}

/// Cop names in the order RuboCop registers them (its `require`s, not default.yml's order:
/// `Registry.global.names` is the did-you-mean dictionary, whose order decides ties), from
/// src/autocorrect/registry.tsv; a name it lacks keeps its default.yml place after them.
fn in_registry_order(mut names: Vec<&'static str>) -> Vec<&'static str> {
    names.sort_by_key(|n| crate::autocorrect::registry_position(n).unwrap_or(usize::MAX));
    names
}

/// The registry: RuboCop's own cops, then those of each native plugin in load order. Plugins load
/// while configs load, before any file is inspected, as RuboCop's global registry fills up then.
pub struct Registry {
    core: LazyLock<Vec<&'static str>>,
    /// (every name once a plugin is loaded, the loaded plugins' gems)
    plugins: RwLock<(Option<&'static [&'static str]>, Vec<&'static str>)>,
}

pub static REGISTRY: Registry = Registry {
    core: LazyLock::new(|| in_registry_order(cop_names(crate::config::DEFAULT_YML))),
    plugins: RwLock::new((None, Vec::new())),
};

impl std::ops::Deref for Registry {
    type Target = [&'static str];
    fn deref(&self) -> &[&'static str] {
        let all = self.plugins.read().unwrap().0.unwrap_or_else(|| self.core.as_slice());
        VIEW.with(|v| v.get()).map_or(all, |n| &all[..n.min(all.len())])
    }
}

thread_local! {
    /// The registry length the file being inspected on this thread sees (see `with_view`).
    static VIEW: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Runs `f` seeing the registry as it was when it held `len` names. RuboCop's registry fills as
/// configs load, which for a subdirectory config is when its first file's turn comes: a file
/// inspected before that does not know the cops of a plugin only that config names. Names are
/// only ever appended, so a prefix is the registry of that moment.
// ponytail: only the registered names; the class-name match of `set_open` is the whole run's.
pub fn with_view<T>(len: Option<usize>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            VIEW.with(|v| v.set(self.0));
        }
    }
    let _restore = Restore(VIEW.with(|v| v.replace(len)));
    f()
}

/// Registers a native plugin's cops (once; later calls are no-ops).
pub fn register_plugin(plugin: &'static NativePlugin) {
    let mut reg = REGISTRY.plugins.write().unwrap();
    if reg.1.contains(&plugin.gem) {
        return;
    }
    reg.1.push(plugin.gem);
    let mut names = reg.0.unwrap_or_else(|| REGISTRY.core.as_slice()).to_vec();
    // A plugin's default.yml also configures some of RuboCop's cops: those are registered already.
    let new: Vec<&'static str> = cop_names(plugin.default_yml).into_iter().filter(|n| !names.contains(n)).collect();
    names.extend(in_registry_order(new));
    internal_affairs_last(&mut names);
    // ponytail: leaks one name list per plugin load (a handful per process at most).
    reg.0 = Some(Vec::leak(names));
}

/// rubocop-internal_affairs' cops are enlisted (Registry#enlist), not registered: they join the
/// registry, and their department the departments, when the enrollment queue is next cleared, after
/// the cops of the plugins the same config loads (#626).
// ponytail: always last, also after a plugin that only a later subdirectory config loads.
fn internal_affairs_last(names: &mut [&'static str]) {
    names.sort_by_key(|n| department_of(n) == "InternalAffairs");
}

/// Registers the cops a plugin rbcop does not implement declares in its default.yml: RuboCop
/// knows them (directives naming them are not unknown), but rbcop cannot run them.
pub fn register_foreign(names: Vec<String>) {
    let mut reg = REGISTRY.plugins.write().unwrap();
    let mut all = reg.0.unwrap_or_else(|| REGISTRY.core.as_slice()).to_vec();
    let new: Vec<String> = without_departments(names).into_iter().filter(|n| !all.contains(&n.as_str())).collect();
    if new.is_empty() {
        return;
    }
    all.extend(new.into_iter().map(|n| &*String::leak(n)));
    internal_affairs_last(&mut all);
    reg.0 = Some(Vec::leak(all));
    drop(reg);
    OPEN.write().unwrap().foreign = true;
}

static CUSTOM: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Registers the cops a `require:`d local Ruby file defines (their classes register in RuboCop when
/// the file loads), each with whether it supports autocorrect.
pub fn register_custom(cops: Vec<(String, bool)>) {
    CUSTOM.write().unwrap().extend(cops.iter().map(|(c, _)| c.clone()));
    set_foreign_autocorrect(cops.iter().filter(|(_, ac)| *ac).map(|(c, _)| c.clone()));
    register_foreign(cops.into_iter().map(|(c, _)| c).collect());
}

/// A cop a `require:`d local Ruby file defines.
pub fn is_custom(cop: &str) -> bool {
    CUSTOM.read().unwrap().iter().any(|c| c == cop)
}

static FOREIGN_AUTOCORRECT: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// The cops of plugins rbcop does not implement that support autocorrect (`extend AutoCorrector`).
pub fn set_foreign_autocorrect(names: impl IntoIterator<Item = String>) {
    FOREIGN_AUTOCORRECT.write().unwrap().extend(names);
}

/// Cop.support_autocorrect? for a foreign plugin's cop.
pub fn foreign_autocorrect(cop: &str) -> bool {
    FOREIGN_AUTOCORRECT.read().unwrap().iter().any(|c| c == cop)
}

/// What rbcop cannot enumerate of RuboCop's registry: cops an unfound gem, a gem without
/// config/default.yml, or a `require:`d local file with no cop class rbcop can read defines
/// (`custom`), and the class names of a gem or local file in which no cop class was found.
struct Open {
    custom: bool,
    foreign: bool,
    constants: Vec<String>,
}

static OPEN: RwLock<Open> = RwLock::new(Open { custom: false, foreign: false, constants: Vec::new() });

/// Set when configs load: see `Open`.
pub fn set_open(custom: bool, constants: &std::collections::HashSet<String>) {
    let mut open = OPEN.write().unwrap();
    open.custom |= custom;
    open.constants = constants.iter().cloned().collect();
}

/// Whether RuboCop's registry would know `cop` (a qualified name): a registered name, or, where
/// rbcop cannot read the cop classes (`Open`), a name made of their class names or any name
/// outside the known departments.
pub fn is_known(cop: &str) -> bool {
    if REGISTRY.contains(&cop) {
        return true;
    }
    let open = OPEN.read().unwrap();
    (!open.constants.is_empty() && cop.split('/').all(|p| open.constants.iter().any(|c| c == p)))
        || (open.custom && cop.contains('/') && !is_department(department_of(cop)))
}

/// A cop RuboCop would run that rbcop has no implementation of (a foreign plugin's or a custom
/// one): its offenses are unknowable, so a directive naming it is never judged redundant.
pub fn is_unrunnable(cop: &str) -> bool {
    static IMPLEMENTED: LazyLock<std::collections::HashSet<&'static str>> =
        LazyLock::new(|| crate::cops::ALL.iter().map(|r| r.name).chain(["Lint/Syntax"]).collect());
    is_known(cop) && !IMPLEMENTED.contains(cop)
}

/// Whether any loaded cop is unrunnable (then a `disable all` range is never judged redundant).
pub fn has_unrunnable() -> bool {
    let open = OPEN.read().unwrap();
    open.custom || open.foreign || !open.constants.is_empty()
}

/// Documentation.builtin?: one of RuboCop's own cops.
pub fn is_builtin(cop: &str) -> bool {
    REGISTRY.core.contains(&cop)
}

/// Whether a native plugin's gem has been loaded (required) in this process.
pub fn plugin_loaded(gem: &str) -> bool {
    REGISTRY.plugins.read().unwrap().1.contains(&gem)
}

/// The loaded native plugins, in load order.
pub fn loaded_plugins() -> Vec<&'static NativePlugin> {
    REGISTRY.plugins.read().unwrap().1.iter().filter_map(|g| native_plugin(g)).collect()
}

/// Whether a cop is registered: RuboCop's own always, a native plugin's once its gem is loaded.
pub fn is_registered(cop: &str) -> bool {
    plugin_of(cop).is_none_or(|p| REGISTRY.plugins.read().unwrap().1.contains(&p.gem))
}

/// The old names of renamed/removed/split/extracted cops (obsoletion.yml `cop_rules`).
pub const LEGACY_COP_NAMES: &[&str] = &[
    "Layout/AlignArguments", "Layout/AlignArray", "Layout/AlignHash", "Layout/AlignParameters", "Layout/IndentArray",
    "Layout/IndentAssignment", "Layout/IndentFirstArgument", "Layout/IndentFirstArrayElement",
    "Layout/IndentFirstHashElement", "Layout/IndentFirstParameter", "Layout/IndentHash", "Layout/IndentHeredoc",
    "Layout/LeadingBlankLines", "Layout/Tab", "Layout/TrailingBlankLines", "Lint/BlockAlignment",
    "Lint/DefEndAlignment", "Lint/DuplicatedKey", "Lint/EndAlignment", "Lint/EndInMethod", "Lint/Eval",
    "Lint/HandleExceptions", "Lint/MultipleCompare", "Lint/StringConversionInInterpolation",
    "Lint/UnneededCopDisableDirective", "Lint/UnneededCopEnableDirective", "Lint/UnneededRequireStatement",
    "Lint/UnneededSplatExpansion", "Metrics/LineLength", "Naming/PredicateName",
    "Naming/UncommunicativeBlockParamName", "Naming/UncommunicativeMethodParamName", "Style/AccessorMethodName",
    "Style/AsciiIdentifiers", "Style/ClassAndModuleCamelCase", "Style/ConstantName", "Style/DeprecatedHashMethods",
    "Style/FileName", "Style/FlipFlop", "Style/MethodCallParentheses", "Style/MethodName", "Style/OpMethod",
    "Style/PredicateName", "Style/SingleSpaceBeforeFirstArg", "Style/UnneededCapitalW", "Style/UnneededCondition",
    "Style/UnneededInterpolation", "Style/UnneededPercentQ", "Style/UnneededSort", "Style/VariableName",
    "Style/VariableNumber", "Gemspec/DateAssignment", "Layout/SpaceAfterControlKeyword",
    "Layout/SpaceBeforeModifierKeyword", "Lint/InvalidCharacterLiteral", "Lint/RescueWithoutErrorClass",
    "Lint/SpaceBeforeFirstArg", "Lint/UselessComparison", "Style/BracesAroundHashParameters",
    "Style/DoubleCopDisableDirective", "Style/MethodMissingSuper", "Style/SpaceAfterControlKeyword",
    "Style/SpaceBeforeModifierKeyword", "Style/TrailingComma", "Style/TrailingCommaInLiteral", "Style/MethodMissing",
    "Performance/*", "Rails/*",
];

/// A cop's department (everything before the last `/`).
pub fn department_of(name: &str) -> &str {
    name.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Registry#department?
pub fn is_department(name: &str) -> bool {
    REGISTRY.iter().any(|c| department_of(c) == name)
}

/// Registry#departments: each department once, in registration order.
pub fn departments() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for c in REGISTRY.iter() {
        let d = department_of(c);
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// Registry#names_for_department: the cops whose department is exactly `dept`.
pub fn names_for_department(dept: &str) -> Vec<&'static str> {
    REGISTRY.iter().copied().filter(|c| department_of(c) == dept).collect()
}

/// Names ordered as `registry.departments.sort.each { with_department(d).sort }`: by department,
/// then by name (`--show-cops`, `--list-enabled-cops-for`).
pub fn sort_by_department(names: &mut [&str]) {
    names.sort_by(|a, b| (department_of(a), *a).cmp(&(department_of(b), *b)));
}

/// Badge.camel_case
fn camel_case(part: &str) -> String {
    if part == "rspec" {
        return "RSpec".to_string();
    }
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]|_[a-z]").unwrap());
    RE.replace_all(part, |c: &regex::Captures| c[0][c[0].len() - 1..].to_uppercase()).into_owned()
}

/// Registry.qualified_cop?: the first department (in registration order) with a cop of this
/// name is the name's own (`RSpec/Output` is not, once rubocop-rails registered Rails/Output).
pub fn is_qualified_cop(name: &str) -> bool {
    let (dept, cop) = (department_of(name), name.rsplit('/').next().unwrap_or(name));
    let registered = |d: &str| REGISTRY.iter().any(|c| department_of(c) == d && c.rsplit('/').next() == Some(cop));
    departments().into_iter().find(|d| registered(d)) == Some(dept)
}

/// Registry#qualified_cop_name(name, nil, warn: false); None on AmbiguousCopName.
pub fn qualified_cop_name(name: &str) -> Option<String> {
    qualify(name, "", None, true, &mut Vec::new()).ok()
}

/// Registry#qualified_cop_name(name, origin, warn:, correct_namespace:). With `warn` (the path as
/// PathUtil.smart_path shows it) the `no department given` and `wrong namespace` warnings are
/// pushed as the lines Registry#emit_warning writes. Err is AmbiguousCopName's message.
/// Registry#unqualified_cop_names: every cop's basename, and RedundantCopDisableDirective. RuboCop
/// memoizes it at its first use: a bare key of the first config file (add_missing_namespaces),
/// or else loading ConfigLoader.default_configuration, whose `AllCops` key goes through
/// qualified_cop_name (`fill_unqualified_cop_names`), on the first plugin integration or once the
/// first config file merges with the defaults. The cops of a plugin that loads later (one only a
/// subdirectory config names), or of a local file required after a plugin, never get the
/// `no department given` warning.
pub fn is_unqualified_cop_name(name: &str) -> bool {
    UNQUALIFIED_COP_NAMES
        .get_or_init(|| {
            REGISTRY.iter().map(|c| c.rsplit('/').next().unwrap_or(c)).chain(["RedundantCopDisableDirective"]).collect()
        })
        .contains(name)
}

static UNQUALIFIED_COP_NAMES: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
    std::sync::OnceLock::new();

/// Memoizes Registry#unqualified_cop_names now, if nothing has yet.
pub fn fill_unqualified_cop_names() {
    is_unqualified_cop_name("");
}

pub fn qualify(
    name: &str,
    origin: &str,
    warn: Option<&str>,
    correct_namespace: bool,
    warnings: &mut Vec<String>,
) -> Result<String, String> {
    // Badge.parse: Ruby's split drops trailing empty fields.
    let mut parts: Vec<String> = name.split('/').map(camel_case).collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    let Some(cop) = parts.pop() else { return Ok(name.to_string()) };
    let qualified = !parts.is_empty();
    let dept = parts.join("/");
    let registered = |d: &str| REGISTRY.iter().any(|c| department_of(c) == d && c.rsplit('/').next() == Some(cop.as_str()));
    // department_missing?: unqualified_cop_names, compared with the name as given.
    if let Some(shown) = warn
        && !qualified
        && is_unqualified_cop_name(name)
    {
        let fix = if origin.ends_with(".rb") { " Run `rubocop -a --only Migration/DepartmentName` to fix." } else { "" };
        warnings.push(format!("{shown}: Warning: no department given for {name}.{fix}"));
    }
    if qualified && registered(&dept) {
        return Ok(name.to_string());
    }
    if qualified && !correct_namespace {
        return Ok(name.to_string());
    }
    let given = if qualified { format!("{dept}/{cop}") } else { cop.clone() };
    let hits: Vec<String> =
        departments().into_iter().filter(|d| registered(d)).map(|d| format!("{d}/{cop}")).collect();
    match hits.as_slice() {
        [] => Ok(name.to_string()),
        [real] => {
            if let Some(shown) = warn
                && qualified
            {
                warnings.push(format!("{shown}: Warning: {given} has the wrong namespace - replace it with {real}"));
            }
            Ok(real.clone())
        }
        many => Err(format!(
            "Ambiguous cop name `{given}` used in {origin} needs department qualifier. Did you mean {}?",
            many.join(" or ")
        )),
    }
}
