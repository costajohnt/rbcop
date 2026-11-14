//! rubocop-sorbet's TargetSorbetVersion: the sorbet-static version in the Bundler lockfile, as
//! `::Bundler.locked_gems.specs.find { |spec| spec.name == "sorbet-static" }&.version` finds it
//! (rescuing GemfileNotFound). Bundler memoizes `locked_gems` for the process and the working
//! directory never changes during a run, so the lookup runs once.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The outcome of the lockfile lookup.
#[derive(Debug, Clone, PartialEq)]
pub enum Locked {
    /// The sorbet-static version, or None when the lockfile has no such spec (or no Gemfile was
    /// found: GemfileNotFound is rescued to nil).
    Version(Option<String>),
    /// A Gemfile but no lockfile: `locked_gems` is nil and `nil.specs` raises NoMethodError.
    NoLockfile,
}

/// Bundler's SharedHelpers#find_gemfile: BUNDLE_GEMFILE when set and non-empty, else `gems.rb`
/// or `Gemfile` (in that order) in the working directory or the nearest directory above it.
fn find_gemfile(env: &dyn Fn(&str) -> Option<String>, cwd: &Path) -> Option<PathBuf> {
    if let Some(given) = env("BUNDLE_GEMFILE").filter(|g| !g.is_empty()) {
        return Some(PathBuf::from(given));
    }
    let mut current = Some(cwd);
    while let Some(dir) = current.filter(|d| d.is_dir()) {
        for name in ["gems.rb", "Gemfile"] {
            let f = dir.join(name);
            if f.is_file() {
                return Some(f);
            }
        }
        current = dir.parent();
    }
    None
}

/// SharedHelpers#default_lockfile: BUNDLE_LOCKFILE when set and non-empty, else `gems.locked`
/// next to a `gems.rb` and `<Gemfile>.lock` otherwise. None: GemfileNotFound.
fn default_lockfile(env: &dyn Fn(&str) -> Option<String>, cwd: &Path) -> Option<PathBuf> {
    if let Some(given) = env("BUNDLE_LOCKFILE").filter(|g| !g.is_empty()) {
        return Some(cwd.join(given));
    }
    let gemfile = cwd.join(find_gemfile(env, cwd)?);
    if gemfile.file_name().is_some_and(|n| n == "gems.rb") {
        return Some(gemfile.with_file_name("gems.locked"));
    }
    let mut lock = gemfile.into_os_string();
    lock.push(".lock");
    Some(PathBuf::from(lock))
}

/// LockfileParser#specs, first `sorbet-static`: spec lines are indented four spaces inside a
/// GIT/GEM/PATH/PLUGIN SOURCE section, `name (version[-platform])`.
fn sorbet_static(lockfile: &str) -> Option<String> {
    let mut in_source = false;
    for line in lockfile.split(['\r', '\n']).filter(|l| !l.is_empty()) {
        if matches!(line, "GIT" | "GEM" | "PATH" | "PLUGIN SOURCE") {
            in_source = true;
        } else if !line.starts_with([' ', '\t']) {
            in_source = false;
        } else if in_source
            && let Some(spec) = line.strip_prefix("    ").filter(|s| !s.starts_with(' '))
        {
            // NAME_VERSION: `(?! )(.*?)(?: \(([^-]*)(?:-(.*))?\))?` over the whole rest.
            let (name, version) = match spec.strip_suffix(')').and_then(|s| s.split_once(" (")) {
                Some((name, v)) => (name, Some(v.split('-').next().unwrap_or(v))),
                None => (spec, None),
            };
            if name == "sorbet-static" {
                return version.map(str::to_string);
            }
        }
    }
    None
}

fn lookup(env: &dyn Fn(&str) -> Option<String>, cwd: &Path) -> Locked {
    let Some(lockfile) = default_lockfile(env, cwd) else { return Locked::Version(None) };
    match std::fs::read(&lockfile) {
        Ok(bytes) if lockfile.is_file() => Locked::Version(sorbet_static(&String::from_utf8_lossy(&bytes))),
        _ => Locked::NoLockfile,
    }
}

/// The lookup for this process (Bundler.locked_gems is memoized the same way).
pub fn locked() -> &'static Locked {
    static LOCKED: OnceLock<Locked> = OnceLock::new();
    LOCKED.get_or_init(|| {
        let cwd = std::env::current_dir().unwrap_or_default();
        lookup(&|k| std::env::var(k).ok(), &cwd)
    })
}

/// Gem::Version#<=>: segments split at dots and digit/letter boundaries, trailing zero segments
/// ignored, a letter segment (prerelease) sorting below any number.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    #[derive(PartialEq)]
    enum Seg {
        N(u128),
        S(String),
    }
    fn segments(v: &str) -> Vec<Seg> {
        static SEG: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"[0-9]+|[a-zA-Z]+").unwrap());
        let mut s: Vec<Seg> = SEG
            .find_iter(v)
            .map(|m| m.as_str().parse().map_or_else(|_| Seg::S(m.as_str().to_string()), Seg::N))
            .collect();
        while matches!(s.last(), Some(Seg::N(0))) {
            s.pop();
        }
        s
    }
    let (a, b) = (segments(a), segments(b));
    for i in 0..a.len().max(b.len()) {
        let ord = match (a.get(i).unwrap_or(&Seg::N(0)), b.get(i).unwrap_or(&Seg::N(0))) {
            (Seg::N(x), Seg::N(y)) => x.cmp(y),
            (Seg::S(x), Seg::S(y)) => x.cmp(y),
            (Seg::S(_), Seg::N(_)) => Ordering::Less,
            (Seg::N(_), Seg::S(_)) => Ordering::Greater,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

/// `sorbet_enabled?`; Err when RuboCop raises (a Gemfile without a lockfile).
pub fn sorbet_enabled() -> Result<bool, ()> {
    match locked() {
        Locked::Version(v) => Ok(v.is_some()),
        Locked::NoLockfile => Err(()),
    }
}

/// `enabled_for_sorbet_static_version?` for a cop's `minimum_target_sorbet_static_version`.
pub fn enabled_for(minimum: &str) -> Result<bool, ()> {
    match locked() {
        Locked::Version(Some(v)) => Ok(compare_versions(minimum, v) != std::cmp::Ordering::Greater),
        Locked::Version(None) => Ok(false),
        Locked::NoLockfile => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfile_lookup() {
        let dir = std::env::temp_dir().join(format!("rbcop-sorbet-lock-{}", std::process::id()));
        let sub = dir.join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        let none = |_: &str| None;
        // No Gemfile anywhere up to / (assumes the temp dir has none above it).
        assert_eq!(lookup(&none, &sub), Locked::Version(None));
        std::fs::write(dir.join("Gemfile"), "").unwrap();
        assert_eq!(lookup(&none, &sub), Locked::NoLockfile);
        std::fs::write(dir.join("Gemfile.lock"), "GEM\n  specs:\n    sorbet-static (0.6.13680-x86_64-linux)\n").unwrap();
        assert_eq!(lookup(&none, &sub), Locked::Version(Some("0.6.13680".into())));
        // gems.rb wins over Gemfile in the same directory and pairs with gems.locked.
        std::fs::write(dir.join("a/gems.rb"), "").unwrap();
        assert_eq!(lookup(&none, &sub), Locked::NoLockfile);
        std::fs::write(dir.join("a/gems.locked"), "GEM\n  specs:\n    sorbet-static (0.5.1)\n").unwrap();
        assert_eq!(lookup(&none, &sub), Locked::Version(Some("0.5.1".into())));
        let gemfile = dir.join("Gemfile").display().to_string();
        let env = move |k: &str| (k == "BUNDLE_GEMFILE").then(|| gemfile.clone());
        assert_eq!(lookup(&env, &sub), Locked::Version(Some("0.6.13680".into())));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(sorbet_static("GEM\n  specs:\n    rake (13.0)\n      sorbet-static (1)\n"), None);
    }

    #[test]
    fn versions() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("0.6.13304", "0.6.13680"), Less);
        assert_eq!(compare_versions("0.5.10210", "0.5.10210.0"), Equal);
        assert_eq!(compare_versions("1.0.a", "1.0"), Less);
    }
}
