//! RuboCop's `Registry.global` (every cop in default.yml, not just the ported ones, plus the cops
//! of the native plugins loaded so far) and `ConfigObsoletion.legacy_cop_names`.

use regex::Regex;
use std::sync::{LazyLock, RwLock};

/// A plugin gem whose cops rbcop implements. Its `config/default.yml` is vendored, so the gem need
/// not be installed: `plugins:` / `require:` of it registers its cops and merges that file.
pub struct NativePlugin {
    pub gem: &'static str,
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
        departments: &["Performance"],
        default_yml: include_str!("../../config/plugins/rubocop-performance.yml"),
        obsoletion: None,
    },
    NativePlugin {
        gem: "rubocop-rails",
        departments: &["Rails"],
        default_yml: include_str!("../../config/plugins/rubocop-rails.yml"),
        obsoletion: Some(include_str!("../../config/plugins/rubocop-rails-obsoletion.yml")),
    },
    NativePlugin {
        gem: "rubocop-minitest",
        departments: &["Minitest"],
        default_yml: include_str!("../../config/plugins/rubocop-minitest.yml"),
        obsoletion: None,
    },
    NativePlugin {
        gem: "rubocop-rspec",
        departments: &["RSpec"],
        default_yml: include_str!("../../config/plugins/rubocop-rspec.yml"),
        obsoletion: Some(include_str!("../../config/plugins/rubocop-rspec-obsoletion.yml")),
    },
    NativePlugin {
        gem: "rubocop-rspec_rails",
        departments: &["RSpecRails"],
        default_yml: include_str!("../../config/plugins/rubocop-rspec_rails.yml"),
        obsoletion: None,
    },
    NativePlugin {
        gem: "rubocop-factory_bot",
        departments: &["FactoryBot"],
        default_yml: include_str!("../../config/plugins/rubocop-factory_bot.yml"),
        obsoletion: None,
    },
    NativePlugin {
        gem: "rubocop-capybara",
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

/// The registry: RuboCop's own cops, then those of each native plugin in load order. Plugins load
/// while configs load, before any file is inspected, as RuboCop's global registry fills up then.
pub struct Registry {
    core: LazyLock<Vec<&'static str>>,
    /// (every name once a plugin is loaded, the loaded plugins' gems)
    plugins: RwLock<(Option<&'static [&'static str]>, Vec<&'static str>)>,
}

pub static REGISTRY: Registry = Registry {
    core: LazyLock::new(|| cop_names(crate::config::DEFAULT_YML)),
    plugins: RwLock::new((None, Vec::new())),
};

impl std::ops::Deref for Registry {
    type Target = [&'static str];
    fn deref(&self) -> &[&'static str] {
        self.plugins.read().unwrap().0.unwrap_or_else(|| self.core.as_slice())
    }
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
    names.extend(new);
    // ponytail: leaks one name list per plugin load (a handful per process at most).
    reg.0 = Some(Vec::leak(names));
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
    reg.0 = Some(Vec::leak(all));
    drop(reg);
    OPEN.write().unwrap().foreign = true;
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

/// What rbcop cannot enumerate of RuboCop's registry: cops a `require:`d local file or an unfound
/// gem defines (`custom`), and class names in loaded plugins' lib/ (cops registered without a
/// default.yml entry).
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

/// Whether RuboCop's registry would know `cop` (a qualified name): a registered name, a name made
/// of plugin class names, or, with custom cops loaded, any name outside the known departments.
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

/// Registry#qualified_cop_name(name, nil, warn: false); None on AmbiguousCopName.
pub fn qualified_cop_name(name: &str) -> Option<String> {
    qualify(name, "", None, true, &mut Vec::new()).ok()
}

/// Registry#qualified_cop_name(name, origin, warn:, correct_namespace:). With `warn` (the path as
/// PathUtil.smart_path shows it) the `no department given` and `wrong namespace` warnings are
/// pushed as the lines Registry#emit_warning writes. Err is AmbiguousCopName's message.
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
    // department_missing?: unqualified_cop_names holds every cop's basename (and
    // RedundantCopDisableDirective), compared with the name as given.
    if let Some(shown) = warn
        && !qualified
        && (name == "RedundantCopDisableDirective" || REGISTRY.iter().any(|c| c.rsplit('/').next() == Some(name)))
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
