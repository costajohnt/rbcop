//! RuboCop's `Registry.global` (every cop in default.yml, not just the ported ones) and
//! `ConfigObsoletion.legacy_cop_names`.

use regex::Regex;
use std::sync::LazyLock;

pub static REGISTRY: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let re = Regex::new(r"(?m)^([A-Z]\w*(?:/\w+)+):").unwrap();
    re.captures_iter(crate::config::DEFAULT_YML).map(|c| c.get(1).unwrap().as_str()).collect()
});

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

fn department_of(name: &str) -> &str {
    name.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Registry#department?
pub fn is_department(name: &str) -> bool {
    REGISTRY.iter().any(|c| department_of(c) == name)
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
    let mut parts: Vec<String> = name.split('/').map(camel_case).collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    let Some(cop) = parts.pop() else { return Some(name.to_string()) };
    let dept = parts.join("/");
    let registered = |d: &str| REGISTRY.iter().any(|c| department_of(c) == d && c.rsplit('/').next() == Some(cop.as_str()));
    if !parts.is_empty() && registered(&dept) {
        return Some(name.to_string());
    }
    let mut depts: Vec<&str> = REGISTRY.iter().map(|c| department_of(c)).collect();
    depts.dedup();
    let hits: Vec<&str> = depts.into_iter().filter(|d| registered(d)).collect();
    match hits.as_slice() {
        [] => Some(name.to_string()),
        [d] => Some(format!("{d}/{cop}")),
        _ => None,
    }
}
