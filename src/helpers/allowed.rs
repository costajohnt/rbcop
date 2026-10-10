//! RuboCop's AllowedMethods and AllowedPattern mixins.

use crate::config::CopConfig;

/// AllowedMethods#allowed_method?: `AllowedMethods` plus the deprecated `IgnoredMethods`.
pub fn allowed_method(cfg: &CopConfig, name: &str) -> bool {
    cfg.strings("AllowedMethods").iter().chain(cfg.strings("IgnoredMethods").iter()).any(|m| m == name)
}

/// AllowedPattern#matches_allowed_pattern?: `AllowedPatterns` plus the deprecated `IgnoredPatterns`.
pub fn matches_allowed_pattern(cfg: &CopConfig, text: &str) -> bool {
    cfg.regexes("AllowedPatterns").iter().chain(cfg.regexes("IgnoredPatterns").iter()).any(|r| r.is_match(text))
}
