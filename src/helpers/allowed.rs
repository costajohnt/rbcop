//! RuboCop's AllowedMethods and AllowedPattern mixins.

use crate::config::CopConfig;
use crate::config::value::{RubyRegex, Value, ruby_regex};

/// The deprecated method keys (`cop_config_deprecated_values`), which both mixins read.
const DEPRECATED_METHODS: [&str; 2] = ["IgnoredMethods", "ExcludedMethods"];

/// `cop_config_deprecated_values.any?(Regexp)`: the deprecated method values then count as
/// patterns, not methods.
fn deprecated_are_patterns(cfg: &CopConfig) -> bool {
    DEPRECATED_METHODS.iter().any(|k| match cfg.get(k) {
        Some(Value::Regex(_)) => true,
        Some(Value::Seq(items)) => items.iter().any(|v| matches!(v, Value::Regex(_))),
        _ => false,
    })
}

/// AllowedMethods#allowed_methods: `AllowedMethods` plus the deprecated `IgnoredMethods` and
/// `ExcludedMethods`, unless one of those holds a Regexp.
pub fn allowed_methods(cfg: &CopConfig) -> Vec<String> {
    let mut out = cfg.strings("AllowedMethods");
    if !deprecated_are_patterns(cfg) {
        DEPRECATED_METHODS.iter().for_each(|k| out.extend(cfg.strings(k)));
    }
    out
}

/// AllowedMethods#allowed_method?.
pub fn allowed_method(cfg: &CopConfig, name: &str) -> bool {
    allowed_methods(cfg).iter().any(|m| m == name)
}

/// The deprecated method values AllowedPattern#allowed_patterns adds, as configured (only when
/// one of them is a Regexp).
pub fn deprecated_pattern_values(cfg: &CopConfig) -> Vec<Value> {
    if !deprecated_are_patterns(cfg) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for key in DEPRECATED_METHODS {
        match cfg.get(key) {
            Some(Value::Seq(items)) => out.extend(items.iter().cloned()),
            None | Some(Value::Null) => {}
            Some(v) => out.push(v.clone()),
        }
    }
    out
}

/// AllowedPattern#allowed_patterns: `AllowedPatterns` plus the deprecated `IgnoredPatterns`, plus
/// the deprecated method keys when one of them holds a Regexp.
pub fn allowed_patterns(cfg: &CopConfig) -> Vec<RubyRegex> {
    let mut out = cfg.regexes("AllowedPatterns");
    out.extend(cfg.regexes("IgnoredPatterns"));
    out.extend(deprecated_pattern_values(cfg).iter().filter_map(|v| match v {
        Value::Regex(r) => ruby_regex(r),
        v => v.as_string().and_then(|s| ruby_regex(&s)),
    }));
    out
}

/// AllowedPattern#matches_allowed_pattern?.
pub fn matches_allowed_pattern(cfg: &CopConfig, text: &str) -> bool {
    allowed_patterns(cfg).iter().any(|r| r.is_match(text))
}
