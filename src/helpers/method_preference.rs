//! RuboCop's MethodPreference mixin (`preferred_method`).

use crate::config::value::{parse_yaml, Map, Value};
use crate::prelude::*;

static DEFAULTS: LazyLock<Map> = LazyLock::new(|| match parse_yaml(crate::config::DEFAULT_YML) {
    Ok(Value::Map(m)) => std::sync::Arc::unwrap_or_clone(m),
    _ => Map::default(),
});

/// `preferred_method(method)`: the cop's PreferredMethods, minus keys that are values the user
/// added (an override like `find: detect` drops the default `detect: find`).
pub fn preferred_method(cx: &Cx, method: &str) -> Option<String> {
    let merged = cx.cfg.get("PreferredMethods").and_then(Value::as_map)?;
    let default = DEFAULTS.map(cx.cop).and_then(|c| c.map("PreferredMethods"));
    let default_values: Vec<String> =
        default.map(|d| d.0.iter().filter_map(|(_, v)| v.as_string()).collect()).unwrap_or_default();
    let overrides: Vec<String> =
        merged.0.iter().filter_map(|(_, v)| v.as_string()).filter(|v| !default_values.contains(v)).collect();
    merged
        .0
        .iter()
        .find(|(k, _)| k == method && !overrides.contains(k))
        .and_then(|(_, v)| v.as_string())
}
