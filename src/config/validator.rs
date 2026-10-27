//! ConfigValidator#validate (plus Config#deprecation_check and ConfigObsoletion) from RuboCop 1.92.1:
//! the checks Config#check runs on every loaded config file, and validate_after_resolution's target
//! Ruby check. Warnings are appended to `out` as Kernel#warn writes them; an error is returned as
//! the CLI prints it (`Error: ...`, painted red by the caller).

use super::value::{Map, Value, parse_yaml};
use super::{base_dir_for, relative_path, target_ruby};
use crate::formatter::Paint;
use crate::helpers::did_you_mean;
use crate::helpers::ruby_string::{float_to_s, inspect as inspect_str};
use regex::Regex;
use std::path::Path;
use std::sync::{Arc, LazyLock};

const COMMON_PARAMS: &[&str] = &[
    "Exclude",
    "Include",
    "Severity",
    "inherit_mode",
    "AutoCorrect",
    "StyleGuide",
    "Details",
    "Enabled",
    "Reference",
    "References",
    "Safe",
    "SafeAutoCorrect",
];
const INTERNAL_PARAMS: &[&str] = &[
    "Description",
    "StyleGuide",
    "Preview",
    "VersionAdded",
    "VersionChanged",
    "VersionRemoved",
    "Reference",
    "References",
    "Safe",
    "SafeAutoCorrect",
];
const NEW_COPS_VALUES: [&str; 3] = ["pending", "disable", "enable"];
/// Cop::Severity::NAMES
const SEVERITIES: [&str; 6] = ["info", "refactor", "convention", "warning", "error", "fatal"];

/// config/obsoletion.yml of the gem (ConfigObsoletion::DEFAULT_RULES_FILE).
static OBSOLETION: LazyLock<Map> = LazyLock::new(|| match parse_yaml(include_str!("../../config/obsoletion.yml")) {
    Ok(Value::Map(m)) => Arc::unwrap_or_clone(m),
    _ => Map::default(),
});

/// ConfigObsoletion#legacy_cop_names: the old names of the cop rules (renamed, removed, split,
/// extracted) of RuboCop and of the plugins loaded so far.
pub fn legacy_cop_names() -> Vec<String> {
    let plugins: Vec<Map> = crate::helpers::cop_registry::loaded_plugins()
        .iter()
        .filter_map(|p| match p.obsoletion.map(parse_yaml) {
            Some(Ok(Value::Map(m))) => Some(Arc::unwrap_or_clone(m)),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for kind in ["renamed", "removed", "split", "extracted"] {
        for rules in std::iter::once(&*OBSOLETION).chain(plugins.iter()) {
            out.extend(rules.map(kind).into_iter().flat_map(|m| m.keys().map(String::from)));
        }
    }
    out
}

/// ConfigObsoletion.deprecated_names_for: the old names of cops renamed to `cop`.
pub fn deprecated_names_for(cop: &str) -> Vec<String> {
    static RENAMED: LazyLock<Vec<(String, String)>> = LazyLock::new(|| {
        let rules = OBSOLETION.map("renamed").map(|m| m.0.clone()).unwrap_or_default();
        rules
            .into_iter()
            .filter_map(|(old, v)| match v {
                Value::Map(m) => m.str("new_name").map(|n| (old, n.to_string())),
                Value::Null => None,
                v => v.as_string().map(|n| (old, n)),
            })
            .collect()
    });
    RENAMED.iter().filter(|(_, new)| new == cop).map(|(old, _)| old.clone()).collect()
}

/// PathUtil.smart_path: relative to pwd when the path (as a string) starts with it.
pub fn smart_path(path: &Path, pwd: &Path) -> String {
    let p = path.display().to_string();
    if p.starts_with(&pwd.display().to_string()) { relative_path(path, pwd) } else { p }
}

/// Ruby's `"#{value}"` for a YAML value.
pub fn to_s(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Str(s) => s.clone(),
        Value::Sym(s) => s.strip_prefix(':').unwrap_or(s).to_string(),
        _ => inspect(v),
    }
}

/// Ruby's `#inspect` (3.4 Hash style) for a YAML value.
// ponytail: a Regexp prints as written (`/x/`), which is Regexp#inspect but not Regexp#to_s.
pub fn inspect(v: &Value) -> String {
    match v {
        Value::Null => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => float_to_s(*f),
        Value::Str(s) => inspect_str(s.as_bytes()),
        Value::Sym(s) => crate::helpers::ruby_string::symbol_inspect(&s.as_bytes()[1..]),
        Value::Regex(s) => s.to_string(),
        Value::Seq(items) => format!("[{}]", items.iter().map(inspect).collect::<Vec<_>>().join(", ")),
        Value::Map(m) => format!(
            "{{{}}}",
            m.0.iter()
                .map(|(k, v)| format!("{} => {}", inspect_str(k.as_bytes()), inspect(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Warnings as `warn` calls: (painted yellow, text). They are painted when written, since Rainbow
/// is switched by `--color` after the pwd config has loaded.
pub type Output = Vec<(bool, String)>;

/// The bytes Kernel#warn writes: a newline unless the (painted) text already ends with one.
pub fn render(out: &[(bool, String)], color: bool) -> String {
    let mut s = String::new();
    for (yellow, text) in out {
        let text = if *yellow { Paint(color).yellow(text) } else { text.clone() };
        s.push_str(&text);
        if !text.ends_with('\n') {
            s.push('\n');
        }
    }
    s
}

/// Rule#to_sentence (an empty list gives `" and "`: `["", nil].join`).
fn to_sentence(items: &[String], connector: &str) -> String {
    match items {
        [one] => one.clone(),
        [init @ .., last] => format!("{} {connector} {last}", init.join(", ")),
        [] => format!(" {connector} "),
    }
}

fn department(name: &str) -> &str {
    name.rsplit_once('/').map_or("", |(d, _)| d)
}

/// `Array(value)` of strings.
fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Seq(items)) => items.iter().filter_map(Value::as_string).collect(),
        Some(v) => v.as_string().into_iter().collect(),
        None => Vec::new(),
    }
}

pub struct Validator<'a> {
    /// ConfigLoader.default_configuration as it stands (plugins integrated so far).
    pub default: &'a Map,
    pub pwd: &'a Path,
    /// Plugin and required gem names (ConfigLoader.loaded_plugins / loaded_features).
    pub loaded_plugins: &'a [String],
    /// `--ignore-unrecognized-cops`
    pub ignore_unrecognized_cops: bool,
    /// A `require:` rbcop cannot load (a local file, or a gem without config/default.yml) may
    /// register cops RuboCop would know; unrecognized names are not reported then.
    pub custom_cops: bool,
    /// Class and module names in loaded plugins' lib/: cops registered without default config.
    pub plugin_constants: &'a std::collections::HashSet<String>,
    pub color: bool,
}

impl Validator<'_> {
    /// Registry.global.map(&:cop_name): every cop in the defaults (core first, then plugins).
    fn registry(&self) -> Vec<&str> {
        self.default.keys().filter(|k| k.contains('/')).collect()
    }

    fn departments(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for c in self.registry() {
            let d = department(c);
            if !out.contains(&d) {
                out.push(d);
            }
        }
        out
    }

    /// Registry#department?
    fn is_department(&self, name: &str) -> bool {
        self.registry().into_iter().any(|c| department(c) == name)
    }

    /// Config#check: deprecation_check, then ConfigValidator#validate. `hash` is the file's config
    /// after inheritance; `path` its absolute path.
    pub fn check(&self, hash: &mut Map, path: &Path, out: &mut Output) -> Result<(), String> {
        // Config#deprecation_check
        if let Some(all) = hash.map_mut("AllCops") {
            for key in ["Exclude", "Include"] {
                let plural = format!("{key}s");
                if !all.truthy(&plural) {
                    continue;
                }
                let v = all.remove(&plural).unwrap();
                all.insert(key, v);
                out.push((false, format!("{} - AllCops/{plural} was renamed to AllCops/{key}", path.display())));
            }
        }
        let shown = smart_path(path, self.pwd);
        self.check_cop_config_value(hash, None)?;
        // reject_conflicting_safe_settings
        for (name, v) in &hash.0 {
            if let Value::Map(m) = v
                && m.get("Safe") == Some(&Value::Bool(false))
                && m.get("SafeAutoCorrect") == Some(&Value::Bool(true))
            {
                return Err(format!(
                    "Error: Unsafe cops cannot have a safe autocorrection (section {name} in {shown})"
                ));
            }
        }
        let (valid, invalid): (Vec<String>, Vec<String>) =
            hash.keys().map(String::from).partition(|k| self.default.contains(k));
        // validate_parameter_shape
        for name in &valid {
            match hash.get(name) {
                Some(Value::Null) => {
                    return Err(format!("Error: empty section {} found in {shown}", inspect_str(name.as_bytes())));
                }
                Some(Value::Map(_)) => {}
                Some(v) => {
                    return Err(format!(
                        "Error: The configuration for {} in {shown} is not a Hash.\n\nFound: {}\n",
                        inspect_str(name.as_bytes()),
                        inspect(v)
                    ));
                }
                None => {}
            }
        }
        self.check_obsoletions(hash, path, &shown, out)?;
        self.alert_about_unrecognized_cops(&invalid, &shown, out)?;
        self.validate_new_cops(hash, &shown)?;
        // validate_fail_level_parameter
        if let Some(level) = hash.map("AllCops").and_then(|a| a.get("FailLevel"))
            && *level != Value::Null
            && !SEVERITIES.contains(&to_s(level).as_str())
        {
            return Err(format!(
                "Error: invalid {} for `FailLevel` found in {shown}\nValid choices are: {}",
                to_s(level),
                SEVERITIES.join(", ")
            ));
        }
        // validate_preview_parameter
        if let Some(preview) = hash.map("AllCops").and_then(|a| a.get("Preview"))
            && !matches!(preview, Value::Null | Value::Bool(_))
        {
            return Err(format!(
                "Error: invalid {} for `Preview` found in {shown}\nValid choices are: true, false",
                to_s(preview)
            ));
        }
        // validate_parameter_names
        for name in &valid {
            let (Some(section), Some(default)) = (hash.map(name), self.default.map(name)) else { continue };
            for param in section.keys() {
                // rubocop-rails appends TargetRailsVersion to ConfigValidator::COMMON_PARAMS.
                let rails_param =
                    param == "TargetRailsVersion" && crate::helpers::cop_registry::plugin_loaded("rubocop-rails");
                if COMMON_PARAMS.contains(&param) || rails_param || default.contains(param) {
                    continue;
                }
                if param == "NewCops" && self.is_department(name) {
                    continue;
                }
                let supported: Vec<&str> = default.keys().filter(|k| !INTERNAL_PARAMS.contains(k)).collect();
                out.push((
                    true,
                    format!(
                        "Warning: {name} does not support {param} parameter.\n\nSupported parameters are:\n\n  - {}\n",
                        supported.join("\n  - ")
                    ),
                ));
            }
        }
        // validate_enforced_styles
        for name in &valid {
            let (Some(section), Some(default)) = (hash.map(name), self.default.map(name)) else { continue };
            for (style_name, style) in &section.0 {
                if !style_name.starts_with("Enforced") {
                    continue;
                }
                // Util.to_supported_styles
                let supported_key = style_name.replacen("Enforced", "Supported", 1).replacen("Style", "Styles", 1);
                // ponytail: a SupportedStyles that is not an Array (never in shipped configs) is skipped.
                let Some(Value::Seq(valid_styles)) = default.get(&supported_key) else { continue };
                if valid_styles.contains(style) {
                    continue;
                }
                let multiple = default.truthy("AllowMultipleStyles")
                    && matches!(style, Value::Seq(items) if items.iter().all(|s| valid_styles.contains(s)));
                if multiple {
                    continue;
                }
                return Err(format!(
                    "Error: invalid {style_name} '{}' for {name} found in {shown}\nValid choices are: {}",
                    to_s(style),
                    valid_styles.iter().map(to_s).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        // validate_syntax_cop: Lint/Syntax config that changes anything in its defaults.
        if let Some(Value::Map(syntax)) = hash.get("Lint/Syntax") {
            let default = self.default.map("Lint/Syntax").cloned().unwrap_or_default();
            if syntax.0.iter().any(|(k, v)| default.get(k) != Some(v)) {
                return Err(format!(
                    "Error: configuration for Lint/Syntax cop found in {shown}\nIt's not possible to disable this cop."
                ));
            }
        }
        // reject_mutually_exclusive_defaults
        if let Some(all) = hash.map("AllCops")
            && all.truthy("DisabledByDefault")
            && all.truthy("EnabledByDefault")
        {
            return Err("Error: Cops cannot be both enabled by default and disabled by default".into());
        }
        Ok(())
    }

    /// check_cop_config_value: string values of Enabled/AutoCorrect/References, depth first.
    fn check_cop_config_value(&self, hash: &Map, parent: Option<&str>) -> Result<(), String> {
        for (key, value) in &hash.0 {
            if let Value::Map(m) = value {
                self.check_cop_config_value(m, Some(key))?;
            }
            let Value::Str(value) = value else { continue };
            let supposed = match key.as_str() {
                "Enabled" if !["pending", "preview", "override_department"].contains(&value.as_str()) => "a boolean",
                "AutoCorrect" if !["always", "contextual", "disabled"].contains(&value.as_str()) => {
                    "`always`, `contextual`, `disabled`, or a boolean"
                }
                "References" => "an array of strings",
                _ => continue,
            };
            let p = Paint(self.color);
            // Rainbow('').reset is a bare reset code.
            let reset = if self.color { "\x1b[0m" } else { "" };
            return Err(format!(
                "Error: {reset}Property {} of {} cop is supposed to be {supposed} and {} is not.",
                p.yellow(key),
                p.yellow(parent.unwrap_or("")),
                p.yellow(value)
            ));
        }
        Ok(())
    }

    /// ConfigObsoletion#reject_obsolete! and ConfigValidator#check_obsoletions.
    fn check_obsoletions(&self, hash: &Map, path: &Path, shown: &str, out: &mut Output) -> Result<(), String> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut add = |warning: bool, msg: String| if warning { warnings.push(msg) } else { errors.push(msg) };
        let cop_msg = |m: String| format!("{m}\n(obsolete configuration found in {shown}, please update it)");
        // CopRule#violated?: the old name, or its bare cop name.
        let has = |old: &str| hash.contains(old) || hash.contains(old.rsplit('/').next().unwrap_or(old));
        let empty = Map::default();
        // ConfigObsoletion.files: RuboCop's rules, then each loaded plugin's (later keys win).
        let rules = |kind: &str| {
            let mut all = OBSOLETION.map(kind).cloned().unwrap_or_default();
            for plugin in crate::helpers::cop_registry::loaded_plugins() {
                if let Some(Ok(Value::Map(m))) = plugin.obsoletion.map(parse_yaml)
                    && let Some(Value::Map(rules)) = m.get(kind)
                {
                    for (k, v) in rules.0.iter() {
                        all.insert(k.clone(), v.clone());
                    }
                }
            }
            all.0
        };
        for (old, v) in rules("renamed") {
            if v == Value::Null || !has(&old) {
                continue;
            }
            let (new, meta) = match &v {
                Value::Map(m) => (m.str("new_name").unwrap_or_default().to_string(), &**m),
                v => (v.as_string().unwrap_or_default(), &empty),
            };
            let (od, nd) = (department(&old), department(&new));
            let moved = od != nd && old.rsplit('/').next() == new.rsplit('/').next();
            let verb = if moved { "moved" } else { "renamed" };
            add(
                meta.str("severity") == Some("warning"),
                cop_msg(format!("The `{old}` cop has been {verb} to `{new}`.")),
            );
        }
        for (old, v) in rules("removed") {
            if v == Value::Null || !has(&old) {
                continue;
            }
            let meta = v.as_map().unwrap_or(&empty);
            let base = format!("The `{old}` cop has been removed");
            let alternatives: Vec<String> =
                strings(meta.get("alternatives")).iter().map(|a| format!("`{a}`")).collect();
            let msg = match meta.str("reason") {
                Some(reason) => format!("{base} since {}.", reason.trim_end_matches('\n')),
                None if alternatives.is_empty() => format!("{base}."),
                None => format!("{base}. Please use {} instead.", to_sentence(&alternatives, "and/or")),
            };
            add(meta.str("severity") == Some("warning"), cop_msg(msg));
        }
        for (old, v) in rules("split") {
            if v == Value::Null || !has(&old) {
                continue;
            }
            let alternatives: Vec<String> =
                strings(v.as_map().and_then(|m| m.get("alternatives"))).iter().map(|a| format!("`{a}`")).collect();
            add(false, cop_msg(format!("The `{old}` cop has been split into {}.", to_sentence(&alternatives, "and"))));
        }
        for (old, v) in rules("extracted") {
            let Some(gem) = v.as_string() else { continue };
            if self.loaded_plugins.contains(&gem) {
                continue;
            }
            // Only whole departments (`Department/*`) are listed.
            let dept = department(&old);
            let affected = hash.keys().filter(|k| *k == dept || k.starts_with(&format!("{dept}/"))).count();
            if affected == 0 {
                continue;
            }
            let name = if affected > 1 { format!("`{dept}` cops have") } else { format!("`{old}` has") };
            add(false, cop_msg(format!("{name} been extracted to the `{gem}` gem.")));
        }
        let mut target = None;
        for kind in ["changed_parameters", "changed_enforced_styles"] {
            // ConfigObsoletion.files: RuboCop's rules, then each loaded plugin's (arrays concatenate).
            let mut entries: Vec<Value> = match OBSOLETION.get(kind) {
                Some(Value::Seq(e)) => e.to_vec(),
                _ => Vec::new(),
            };
            for plugin in crate::helpers::cop_registry::loaded_plugins() {
                if let Some(Ok(Value::Map(m))) = plugin.obsoletion.map(parse_yaml)
                    && let Some(Value::Seq(e)) = m.get(kind)
                {
                    entries.extend(e.iter().cloned());
                }
            }
            for entry in entries.iter() {
                let Value::Map(meta) = entry else { continue };
                for cop in strings(meta.get("cops")) {
                    for parameter in strings(meta.get("parameters")) {
                        let Some(section) = hash.map(&cop) else { continue };
                        if !section.contains(&parameter) {
                            continue;
                        }
                        if let Some(min) = meta.get("minimum_ruby_version") {
                            let min = match min {
                                Value::Float(f) => *f,
                                Value::Int(i) => *i as f64,
                                _ => 0.0,
                            };
                            let version = *target.get_or_insert_with(|| {
                                let all = hash.map("AllCops").cloned().unwrap_or_default();
                                target_ruby::detect(&all, &base_dir_for(Some(path)))
                            });
                            if version < min {
                                continue;
                            }
                        }
                        let warning = meta.str("severity") == Some("warning");
                        let alternative = meta.str("alternative").map(|a| a.trim_end_matches('\n').to_string());
                        let reason =
                            meta.str("reason").map(|r| r.trim_end_matches('\n').to_string()).unwrap_or_default();
                        if kind == "changed_enforced_styles" {
                            let value = meta.get("value").cloned().unwrap_or(Value::Null);
                            if section.get(&parameter) != Some(&value) {
                                continue;
                            }
                            let value = to_s(&value);
                            let base = format!("obsolete `{parameter}: {value}` (for `{cop}`) found in {shown}");
                            add(
                                warning,
                                match alternative {
                                    Some(a) => format!(
                                        "{base}\n`{parameter}: {value}` has been renamed to `{parameter}: {a}`."
                                    ),
                                    None => format!("{base}\n{reason}"),
                                },
                            );
                            continue;
                        }
                        let base = format!("obsolete parameter `{parameter}` (for `{cop}`) found in {shown}");
                        let alternatives: Vec<String> =
                            strings(meta.get("alternatives")).iter().map(|a| format!("`{a}`")).collect();
                        add(
                            warning,
                            match alternative {
                                Some(a) => format!("{base}\n`{parameter}` has been renamed to `{a}`."),
                                None if !alternatives.is_empty() => format!(
                                    "{base}\n`{parameter}` has been renamed to {}.",
                                    to_sentence(&alternatives, "and/or")
                                ),
                                None => format!("{base}\n{reason}"),
                            },
                        );
                    }
                }
            }
        }
        if !errors.is_empty() {
            return Err(format!("Error: {}", errors.join("\n")));
        }
        if !warnings.is_empty() {
            out.push((true, format!("Warning: {}", warnings.join("\n"))));
        }
        Ok(())
    }

    /// alert_about_unrecognized_cops: an error, or a warning under --ignore-unrecognized-cops.
    fn alert_about_unrecognized_cops(&self, invalid: &[String], shown: &str, out: &mut Output) -> Result<(), String> {
        if self.custom_cops {
            return Ok(());
        }
        let registry = self.registry();
        let departments = self.departments();
        let mut unknown = Vec::new();
        for name in invalid {
            // Registry#contains_cop_matching?, ConfigObsoletion.deprecated_cop_name?, inherit_mode.
            if registry.iter().any(|c| c == name || department(c) == name)
                || crate::helpers::cop_registry::LEGACY_COP_NAMES.contains(&name.as_str())
                || name == "inherit_mode"
                || name.split('/').all(|part| self.plugin_constants.contains(part))
            {
                continue;
            }
            // NameSimilarity.find_similar_names(name, departments + cop names)
            let dictionary: Vec<&str> =
                departments.iter().chain(registry.iter()).copied().filter(|d| d != name).collect();
            let similar = did_you_mean::correct(name, &dictionary);
            let suggestion = if !similar.is_empty() {
                format!("Did you mean `{}`?", similar.join("`, `"))
            } else {
                let nested: Vec<&str> =
                    departments.iter().copied().filter(|d| d.starts_with(&format!("{name}/"))).collect();
                if nested.is_empty() {
                    String::new()
                } else {
                    format!("{name} is not a department. Use `{}`.", nested.join("`, `"))
                }
            };
            let msg = format!("unrecognized cop or department {name} found in {shown}\n{suggestion}");
            unknown.push(msg.trim_end().to_string());
        }
        if unknown.is_empty() {
            return Ok(());
        }
        if self.ignore_unrecognized_cops {
            out.push((true, "The following cops or departments are not recognized and will be ignored:".into()));
            out.push((false, unknown.join("\n")));
            return Ok(());
        }
        Err(format!("Error: {}", unknown.join("\n")))
    }

    /// validate_new_cops_parameter: `AllCops: NewCops` must be pending/disable/enable, a
    /// department's may also be a version.
    fn validate_new_cops(&self, hash: &Map, shown: &str) -> Result<(), String> {
        if let Some(v) = hash.map("AllCops").and_then(|a| a.get("NewCops"))
            && *v != Value::Null
            && !matches!(v, Value::Str(s) if NEW_COPS_VALUES.contains(&s.as_str()))
        {
            return Err(format!(
                "Error: invalid {} for `NewCops` found in {shown}\nValid choices are: {}",
                to_s(v),
                NEW_COPS_VALUES.join(", ")
            ));
        }
        let version = Regex::new(r"\A\d+(\.\d+)*\z").unwrap();
        for (name, section) in &hash.0 {
            let Value::Map(section) = section else { continue };
            if name == "AllCops" || !self.is_department(name) {
                continue;
            }
            let ok = match section.get("NewCops") {
                None | Some(Value::Null | Value::Int(_) | Value::Float(_)) => true,
                Some(Value::Str(s)) => NEW_COPS_VALUES.contains(&s.as_str()) || version.is_match(s),
                Some(_) => false,
            };
            if !ok {
                return Err(format!(
                    "Error: invalid {} for `NewCops` found in {shown}\nValid choices for a department are: {}, or a \
                     version string like '1.50'",
                    to_s(section.get("NewCops").unwrap()),
                    NEW_COPS_VALUES.join(", ")
                ));
            }
        }
        Ok(())
    }
}

/// ConfigValidator#check_target_ruby (validate_after_resolution) for a loaded config file.
pub fn check_target_ruby(hash: &Map, path: &Path, pwd: &Path) -> Result<(), String> {
    let all = hash.map("AllCops").cloned().unwrap_or_default();
    let (version, source) = target_ruby::detect_source(&all, &base_dir_for(Some(path)), &smart_path(path, pwd));
    if target_ruby::KNOWN_RUBIES.contains(&version) {
        return Ok(());
    }
    const OBSOLETE: [(f64, &str); 8] = [
        (1.9, "0.41"),
        (2.0, "0.50"),
        (2.1, "0.57"),
        (2.2, "0.68"),
        (2.3, "0.81"),
        (2.4, "1.12"),
        (2.5, "1.28"),
        (2.6, "1.50"),
    ];
    let v = float_to_s(version);
    let msg = match OBSOLETE.iter().find(|(o, _)| *o == version) {
        Some((_, last)) => format!(
            "RuboCop found unsupported Ruby version {v} in {source}. {v}-compatible analysis was dropped after version \
             {last}."
        ),
        None => format!("RuboCop found unknown Ruby version {v} in {source}."),
    };
    let supported: Vec<String> = target_ruby::KNOWN_RUBIES.iter().map(|v| float_to_s(*v)).collect();
    Err(format!("Error: {msg}\nSupported versions: {}", supported.join(", ")))
}

/// Team#validate_config: the `validate_config` of each cop the team mobilizes (`team` says which),
/// in registry order; the first ValidationError wins. RuboCop 1.92.1 has four.
pub fn cop_validate_config(cfg: &super::Resolved, team: impl Fn(&str) -> bool) -> Result<(), String> {
    const MAGIC: &str = "Layout/EmptyLineAfterMagicComment";
    if team(MAGIC) {
        let n = cfg.cop(MAGIC).params.get("NumberOfEmptyLines").cloned().unwrap_or(Value::Null);
        if !matches!(n, Value::Int(i) if i > 0) {
            return Err(format!(
                "Error: The `{MAGIC}` cop only accepts a positive integer for its `NumberOfEmptyLines` configuration \
                 parameter, but `{}` was given.",
                inspect(&n)
            ));
        }
    }
    for cop in ["Layout/MultilineMethodCallIndentation", "Layout/MultilineOperationIndentation"] {
        let c = cfg.cop(cop);
        if team(cop) && c.str("EnforcedStyle") == Some("aligned") && c.params.truthy("IndentationWidth") {
            return Err(format!(
                "Error: The `{cop}` cop only accepts an `IndentationWidth` configuration parameter when \
                 `EnforcedStyle` is `indented`."
            ));
        }
    }
    const PREDICATE: &str = "Naming/PredicatePrefix";
    if team(PREDICATE) {
        let p = &cfg.cop(PREDICATE).params;
        // `forbidden_prefixes.each`: on a scalar a NoMethodError ends the run (exit 2; rbcop prints
        // the message without the backtrace). `predicate_prefixes.include?` is a substring test on
        // a String NamePrefix.
        // A key set to `~` is gone after the merge: nil.
        let class = |v: Option<&Value>| match v {
            Some(Value::Str(_)) => Some("an instance of String"),
            Some(Value::Sym(_)) => Some("an instance of Symbol"),
            Some(Value::Int(_)) => Some("an instance of Integer"),
            Some(Value::Float(_)) => Some("an instance of Float"),
            Some(Value::Null) | None => Some("nil"),
            Some(Value::Bool(true)) => Some("true"),
            Some(Value::Bool(false)) => Some("false"),
            Some(Value::Seq(_) | Value::Map(_) | Value::Regex(_)) => None,
        };
        if let Some(class) = class(p.get("ForbiddenPrefixes")) {
            return Err(format!("undefined method 'each' for {class}"));
        }
        let included = |f: &Value| match p.get("NamePrefix") {
            Some(Value::Seq(names)) => Ok(names.contains(f)),
            Some(Value::Str(names)) => Ok(f.as_string().is_some_and(|f| names.contains(f.as_str()))),
            Some(Value::Map(names)) => Ok(f.as_string().is_some_and(|f| names.contains(&f))),
            v => match class(v) {
                Some(class) => Err(format!("undefined method 'include?' for {class}")),
                None => Ok(true),
            },
        };
        if let Some(Value::Seq(forbidden)) = p.get("ForbiddenPrefixes") {
            for f in forbidden.iter() {
                if !included(f)? {
                    return Err(format!(
                        "Error: The `{PREDICATE}` cop is misconfigured. Prefix {} must be included in NamePrefix because \
                         it is included in ForbiddenPrefixes.",
                        to_s(f)
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruby_inspect_and_sentence() {
        let v = Value::Seq(vec![Value::Str("a".into()), Value::Int(1), Value::Null, Value::Sym(":s".into())].into());
        assert_eq!(inspect(&v), r#"["a", 1, nil, :s]"#);
        assert_eq!(to_s(&Value::Sym(":s".into())), "s");
        // Symbol#inspect quotes a name that is no identifier (`:"2"`)
        assert_eq!(inspect(&Value::Sym(":2".into())), r#":"2""#);
        let mut m = Map::default();
        m.insert("k", Value::Bool(true));
        assert_eq!(inspect(&Value::Map(m.into())), r#"{"k" => true}"#);
        let items: Vec<String> = ["`A`", "`B`", "`C`"].iter().map(|s| s.to_string()).collect();
        assert_eq!(to_sentence(&items, "and/or"), "`A`, `B` and/or `C`");
        assert!(OBSOLETION.map("renamed").is_some_and(|m| m.contains("Layout/AlignHash")));
    }
}
