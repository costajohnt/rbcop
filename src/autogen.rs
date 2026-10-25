//! What `--auto-gen-config` learns from the cops while they run: DisabledConfigFormatter's
//! `config_to_allow_offenses` and `detected_styles`, and ExcludeLimit's per-cop values.
//!
//! RuboCop keeps this in process-global state that each cop updates as it inspects, file after
//! file. rbcop lints files in parallel, so a cop records its updates for the file being inspected
//! on this thread (`record`), and the reporting loop replays each file's updates in file order
//! (`replay`), which reproduces RuboCop's sequential state.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

static ON: AtomicBool = AtomicBool::new(false);

/// `--auto-gen-config` (or the DisabledConfigFormatter otherwise) is collecting.
pub fn enable() {
    ON.store(true, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// A `config_to_allow_offenses` value. `Nil` is what a style given as a String (not a Symbol)
/// becomes (ConfigurableEnforcedStyle's SYMBOL_TO_STRING_CACHE maps only Symbols).
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Str(String),
    Int(i64),
    Bool(bool),
    Nil,
}

impl std::fmt::Display for Val {
    /// `#{value}`
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Val::Str(s) => f.write_str(s),
            Val::Int(i) => write!(f, "{i}"),
            Val::Bool(b) => write!(f, "{b}"),
            Val::Nil => Ok(()),
        }
    }
}

/// A detected style: Some(name) for a Symbol, None (nil) for a String.
pub type Style = Option<String>;

/// One cop's state.
#[derive(Clone, Debug, Default)]
pub struct CopState {
    /// `config_to_allow_offenses[cop_name]`, in insertion order.
    pub cfg: Vec<(String, Val)>,
    /// `detected_styles[cop_name]`
    pub detected: Option<Vec<Style>>,
    /// ExcludeLimit: every value written for each parameter, in the order the parameters were
    /// first written.
    pub limits: Vec<(String, i64)>,
    /// ArrayMinSize's `self.class.largest_brackets` (a class variable, so it outlives the file;
    /// None is -Infinity).
    pub largest_brackets: Option<i64>,
    /// ArrayMinSize's `@smallest_percent`, of the cop instance inspecting the file (None is
    /// Infinity).
    pub smallest_percent: Option<i64>,
}

impl CopState {
    pub fn get(&self, key: &str) -> Option<&Val> {
        self.cfg.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// `cfg[key] = value`
    pub fn set(&mut self, key: &str, value: Val) {
        match self.cfg.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => self.cfg.push((key.to_string(), value)),
        }
    }

    /// `config_to_allow_offenses['Enabled'] == false`
    pub fn no_acceptable_style(&self) -> bool {
        self.get("Enabled") == Some(&Val::Bool(false))
    }

    /// `self.config_to_allow_offenses = { 'Enabled' => false }`
    pub fn no_acceptable_style_now(&mut self) {
        self.cfg = vec![("Enabled".into(), Val::Bool(false))];
    }

    /// ConfigurableEnforcedStyle#style_detected for `param` (style_parameter_name).
    pub fn style_detected(&mut self, param: &str, detected: &[Style]) {
        if self.no_acceptable_style() {
            return;
        }
        let updated: Vec<Style> = match &self.detected {
            // `detected_style & detected_as_strings` (its shortcut for a lone matching Symbol gives
            // the same list).
            // ponytail: the shortcut also keeps `[nil]` against an Array of possibilities; no cop
            // mixes String styles with ambiguous detections.
            Some(list) => {
                let mut out: Vec<Style> = Vec::new();
                for s in list {
                    if detected.contains(s) && !out.contains(s) {
                        out.push(s.clone());
                    }
                }
                out
            }
            None => detected.to_vec(),
        };
        if updated.is_empty() {
            self.no_acceptable_style_now();
        } else {
            let first = updated[0].clone().map_or(Val::Nil, Val::Str);
            self.detected = Some(updated);
            self.set(param, first);
        }
    }

    /// ArrayMinSize#array_style_detected (`style` is `brackets` or `percent`).
    pub fn array_style_detected(&mut self, style: &str, size: i64) {
        if self.no_acceptable_style() {
            return;
        }
        if style == "brackets" && self.largest_brackets.is_none_or(|l| size > l) {
            self.largest_brackets = Some(size);
        }
        if style == "percent" && self.smallest_percent.is_none_or(|s| size < s) {
            self.smallest_percent = Some(size);
        }
        match self.get("EnforcedStyle") {
            Some(Val::Str(s)) if s == style => {}
            None => self.set("EnforcedStyle", Val::Str(style.to_string())),
            _ => match (self.smallest_percent, self.largest_brackets) {
                (Some(s), Some(l)) if s <= l => self.no_acceptable_style_now(),
                (_, l) => {
                    self.set("EnforcedStyle", Val::Str("percent".into()));
                    self.set("MinSize", Val::Int(l.map_or(i64::MIN, |l| l + 1)));
                }
            },
        }
    }

    /// ExcludeLimit: `self.<param> = value` appends the value to the cop's file for the parameter.
    pub fn limit(&mut self, param: &str, value: i64) {
        match self.limits.iter_mut().find(|(p, _)| p == param) {
            Some((_, v)) => *v = (*v).max(value),
            None => self.limits.push((param.to_string(), value)),
        }
    }
}

type Update = Box<dyn FnOnce(&mut CopState) + Send>;

/// One file's updates: (cop, update), in order.
pub type Updates = Vec<(&'static str, Update)>;

thread_local! {
    /// The updates of the file being inspected on this thread, in order.
    static FILE: RefCell<Vec<(&'static str, Update)>> = const { RefCell::new(Vec::new()) };
}

/// A cop's update of its state while it inspects the current file (only while collecting).
pub fn record(cop: &'static str, f: impl FnOnce(&mut CopState) + Send + 'static) {
    if on() {
        FILE.with(|u| u.borrow_mut().push((cop, Box::new(f))));
    }
}

/// The updates recorded on this thread since the last call (one file's).
pub fn take_file() -> Updates {
    FILE.with(|u| std::mem::take(&mut *u.borrow_mut()))
}

static STATE: LazyLock<Mutex<HashMap<String, CopState>>> = LazyLock::new(Default::default);

/// Apply one file's updates, in the order its cops made them.
pub fn replay(updates: Updates) {
    let mut state = STATE.lock().unwrap();
    // Each file is inspected by new cop instances.
    for s in state.values_mut() {
        s.smallest_percent = None;
    }
    for (cop, f) in updates {
        f(state.entry(cop.to_string()).or_default());
    }
}

/// The state collected so far for `cop`.
pub fn state(cop: &str) -> CopState {
    STATE.lock().unwrap().get(cop).cloned().unwrap_or_default()
}

/// Forget everything (each `--auto-gen-config` phase starts from the state the previous one left,
/// as RuboCop's process-global state does, so this is only for tests).
pub fn reset() {
    STATE.lock().unwrap().clear();
}

/// The `--auto-gen-config` command-line options.
#[derive(Clone, Debug, Default)]
pub struct GenOptions {
    /// `--auto-gen-only-exclude` / `--no-auto-gen-only-exclude`
    pub only_exclude: Option<bool>,
    pub disable_pending_cops: bool,
    pub enable_pending_cops: bool,
    /// `--preview` / `--no-preview`
    pub preview: Option<bool>,
    /// `--exclude-limit COUNT`, as given.
    pub exclude_limit: Option<String>,
    /// `--no-exclude-limit`
    pub no_exclude_limit: bool,
    /// `--[no-]offense-counts`
    pub offense_counts: Option<bool>,
    /// `--[no-]auto-gen-timestamp`
    pub timestamp: Option<bool>,
    /// `--[no-]auto-gen-enforced-style`
    pub enforced_style: Option<bool>,
}

impl GenOptions {
    /// `Integer(@exclude_limit_option)`
    pub fn exclude_limit_value(&self) -> Option<i64> {
        self.exclude_limit.as_deref().and_then(crate::config::value::ruby_integer)
    }

    /// DisabledConfigFormatter#command
    pub fn command(&self) -> String {
        let mut c = String::from("rubocop --auto-gen-config");
        if self.only_exclude == Some(true) {
            c.push_str(" --auto-gen-only-exclude");
        }
        if self.disable_pending_cops {
            c.push_str(" --disable-pending-cops");
        }
        if self.enable_pending_cops {
            c.push_str(" --enable-pending-cops");
        }
        match self.preview {
            Some(true) => c.push_str(" --preview"),
            Some(false) => c.push_str(" --no-preview"),
            None => {}
        }
        if self.no_exclude_limit {
            c.push_str(" --no-exclude-limit");
        } else if let Some(n) = self.exclude_limit_value() {
            c.push_str(&format!(" --exclude-limit {n}"));
        }
        if self.offense_counts == Some(false) {
            c.push_str(" --no-offense-counts");
        }
        if self.timestamp == Some(false) {
            c.push_str(" --no-auto-gen-timestamp");
        }
        if self.enforced_style == Some(false) {
            c.push_str(" --no-auto-gen-enforced-style");
        }
        c
    }
}

/// What the todo formatter needs from the command line.
#[derive(Clone, Default)]
pub struct GenContext {
    pub opts: GenOptions,
    /// The options configs load with, for the fresh ConfigStore the formatter opens.
    pub store_opts: crate::config::Options,
    pub cop_names: Vec<&'static str>,
    /// `--stderr`: `puts` goes to stderr.
    pub stderr: bool,
    /// Where the todo may be written (autocorrect::write_roots).
    pub roots: Vec<std::path::PathBuf>,
}

static CONTEXT: Mutex<Option<GenContext>> = Mutex::new(None);

pub fn set_gen_context(c: GenContext) {
    *CONTEXT.lock().unwrap() = Some(c);
}

pub fn gen_context() -> GenContext {
    CONTEXT.lock().unwrap().clone().unwrap_or_default()
}

/// `ConfigStore.new.for(Dir.pwd)`: a store that has seen no command-line option.
pub fn fresh_pwd_config() -> Option<std::sync::Arc<crate::config::Resolved>> {
    let g = gen_context();
    let opts = crate::config::Options { config_file: None, force_default_config: false, ..g.store_opts };
    let mut store = crate::config::Store::new(opts, g.cop_names).ok()?;
    let cwd = store.cwd().to_path_buf();
    store.for_dir(&cwd).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Style {
        Some(x.into())
    }

    #[test]
    fn style_detection_narrows_then_gives_up() {
        let mut c = CopState::default();
        c.style_detected("EnforcedStyle", &[s("a"), s("b")]);
        assert_eq!(c.get("EnforcedStyle"), Some(&Val::Str("a".into())));
        c.style_detected("EnforcedStyle", &[s("b")]);
        assert_eq!(c.get("EnforcedStyle"), Some(&Val::Str("b".into())));
        c.style_detected("EnforcedStyle", &[s("a")]);
        assert!(c.no_acceptable_style());
        c.style_detected("EnforcedStyle", &[s("a")]);
        assert_eq!(c.cfg, vec![("Enabled".to_string(), Val::Bool(false))]);
    }

    #[test]
    fn limits_keep_the_maximum() {
        let mut c = CopState::default();
        c.limit("Max", 3);
        c.limit("Max", 9);
        c.limit("Max", 4);
        assert_eq!(c.limits, vec![("Max".to_string(), 9)]);
    }
}
