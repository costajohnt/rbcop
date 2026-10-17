//! Cops rbcop cannot run (plugin cops it does not implement, cops from `require:`d Ruby files):
//! a one-line notice on stderr, or with RBCOP_FALLBACK=1 a RuboCop subprocess that runs exactly
//! those cops, whose offenses are merged into rbcop's report.

use rbcop::autocorrect::Mode;
use rbcop::config::{Options, Resolved};
use rbcop::cop::{Offense, Severity};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

const FORMATTER: &str = include_str!("fallback_formatter.rb");

/// RBCOP_FALLBACK=1 asks for the RuboCop subprocess.
pub fn enabled() -> bool {
    std::env::var_os("RBCOP_FALLBACK").is_some_and(|v| v == "1")
}

/// Registered cops (Registry.global, a plugin's cops with or without a default.yml entry) enabled
/// and selected in any of `configs` that rbcop does not implement. Custom cops are counted as
/// required code.
pub fn unimplemented<'c>(
    configs: impl IntoIterator<Item = &'c Resolved>,
    implemented: &HashSet<&str>,
    selected: impl Fn(&str, &Resolved) -> bool,
) -> BTreeSet<String> {
    let registry = rbcop::helpers::cop_registry::REGISTRY.to_vec();
    let mut out = BTreeSet::new();
    for cfg in configs {
        for &name in &registry {
            if !implemented.contains(name)
                && !rbcop::helpers::cop_registry::is_custom(name)
                && !out.contains(name)
                && selected(name, cfg)
                && cfg.cop_enabled(name)
            {
                out.insert(name.to_string());
            }
        }
    }
    out
}

/// `rbcop: not running 3 cops it does not implement (Rails: 2, RSpec: 1); set RBCOP_FALLBACK=1 ...`
pub fn notice(skipped: &BTreeSet<String>, required_code: bool) -> String {
    let mut parts = Vec::new();
    if !skipped.is_empty() {
        let mut depts: BTreeMap<&str, usize> = BTreeMap::new();
        for cop in skipped {
            *depts.entry(cop.rsplit_once('/').map_or("", |(d, _)| d)).or_default() += 1;
        }
        let depts: Vec<String> = depts.iter().map(|(d, n)| format!("{d}: {n}")).collect();
        let n = skipped.len();
        parts.push(format!("{n} cop{} it does not implement ({})", if n == 1 { "" } else { "s" }, depts.join(", ")));
    }
    if required_code {
        parts.push("cops from Ruby code the config requires".into());
    }
    format!("rbcop: not running {}; set RBCOP_FALLBACK=1 to run them with RuboCop", parts.join(" or "))
}

/// What the RuboCop subprocess needs from the command line.
pub struct Child<'a> {
    pub opts: &'a Options,
    pub except: &'a [String],
    pub only_guide: bool,
    pub force_exclusion: bool,
    pub autocorrect: Option<Mode>,
}

/// Runs RuboCop (`RBCOP_RUBOCOP`, default `rubocop`) on `files` with every cop rbcop implements
/// excluded, and returns its offenses by file. Under -a/-A RuboCop corrects (and writes) first, so
/// rbcop then inspects the result; its offenses refer to the source before correction.
pub fn run(
    files: &[PathBuf],
    cwd: &Path,
    implemented: &[&str],
    c: Child,
) -> Result<HashMap<PathBuf, Vec<Offense>>, String> {
    let mut out = HashMap::new();
    if files.is_empty() {
        return Ok(out);
    }
    let program = std::env::var("RBCOP_RUBOCOP").unwrap_or_else(|_| "rubocop".into());
    let mut words = program.split_whitespace();
    let Some(bin) = words.next() else { return Err("rbcop: RBCOP_RUBOCOP is empty".into()) };
    // A fresh file (create_new) so nothing else in the temp dir can choose the Ruby that runs.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let fmt = std::env::temp_dir().join(format!("rbcop-fallback-{}-{nanos}.rb", std::process::id()));
    let mut f = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(&fmt)
        .map_err(|e| format!("rbcop: RBCOP_FALLBACK: {}: {e}", fmt.display()))?;
    f.write_all(FORMATTER.as_bytes()).map_err(|e| e.to_string())?;
    drop(f);

    // Lint/Syntax cannot be excluded; the formatter drops its offenses.
    let except: Vec<&str> = implemented
        .iter()
        .copied()
        .filter(|&n| n != "Lint/Syntax")
        .chain(c.except.iter().map(String::as_str))
        .collect();
    let mut cmd = Command::new(bin);
    cmd.args(words)
        .args(["--cache", "false", "--require"])
        .arg(&fmt)
        .args(["--format", "RbcopFallbackFormatter", "--except", &except.join(",")])
        .current_dir(cwd)
        .env_remove("RBCOP_FALLBACK")
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    let o = c.opts;
    if let Some(cfg) = &o.config_file {
        cmd.arg("--config").arg(cfg);
    }
    if !o.only.is_empty() {
        cmd.args(["--only", &o.only.join(",")]);
    }
    // The pending-cops banner is rbcop's to print; pending cops stay disabled either way.
    cmd.arg(if o.enable_pending_cops { "--enable-pending-cops" } else { "--disable-pending-cops" });
    for (on, flag) in [
        (o.ignore_parent_exclusion, "--ignore-parent-exclusion"),
        (o.ignore_unrecognized_cops, "--ignore-unrecognized-cops"),
        (o.safe, "--safe"),
        (o.all_cops == Some(true), "--enable-all-cops"),
        (o.all_cops == Some(false), "--disable-all-cops"),
        (o.preview == Some(true), "--preview"),
        (o.preview == Some(false), "--no-preview"),
        (c.only_guide, "--only-guide-cops"),
        (c.force_exclusion, "--force-exclusion"),
        (c.autocorrect == Some(Mode::Safe), "--autocorrect"),
        (c.autocorrect == Some(Mode::All), "--autocorrect-all"),
    ] {
        if on {
            cmd.arg(flag);
        }
    }
    // ponytail: one argv for every file; ARG_MAX (1 MiB on macOS) fits ~10k typical paths.
    cmd.arg("--").args(files);
    // Under -a/-A the offsets refer to the source RuboCop read, which it then rewrites.
    let originals: HashMap<&PathBuf, Arc<str>> = if c.autocorrect.is_some() {
        files
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok().map(|t| (p, Arc::from(t.replace("\r\n", "\n")))))
            .collect()
    } else {
        HashMap::new()
    };
    let result = cmd.output();
    let _ = std::fs::remove_file(&fmt);
    let result = result.map_err(|e| format!("rbcop: RBCOP_FALLBACK: cannot run {program}: {e}"))?;
    // 0: no offenses, 1: offenses; anything else is RuboCop failing (its stderr was passed through).
    if !matches!(result.status.code(), Some(0 | 1)) {
        return Err(format!("rbcop: RBCOP_FALLBACK: {program} failed ({})", result.status));
    }
    let mut names: HashMap<String, &'static str> = HashMap::new();
    for line in String::from_utf8_lossy(&result.stdout).lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [path, start, end, severity, correctable, corrected, global, cop, message] = f[..] else {
            return Err(format!("rbcop: RBCOP_FALLBACK: unexpected output from {program}: {line}"));
        };
        let path = PathBuf::from(unescape(path));
        // Cop names live for the whole run.
        let cop: &'static str = names.entry(cop.to_string()).or_insert_with(|| Box::leak(cop.into()));
        let offense = Offense {
            start: start.parse().unwrap_or(0),
            end: end.parse().unwrap_or(0),
            cop,
            message: unescape(message),
            severity: Severity::parse(severity).unwrap_or(Severity::Convention),
            correctable: correctable == "1",
            global: global == "1",
            corrected: corrected == "1",
            source: originals.get(&path).cloned(),
        };
        out.entry(path).or_insert_with(Vec::new).push(offense);
    }
    Ok(out)
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_groups_by_department() {
        let cops: BTreeSet<String> = ["Rails/A", "Rails/B", "RSpec/C"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            notice(&cops, true),
            "rbcop: not running 3 cops it does not implement (RSpec: 1, Rails: 2) or cops from Ruby code the \
             config requires; set RBCOP_FALLBACK=1 to run them with RuboCop"
        );
        assert_eq!(
            notice(&BTreeSet::new(), true),
            "rbcop: not running cops from Ruby code the config requires; set RBCOP_FALLBACK=1 to run them with RuboCop"
        );
    }

    #[test]
    fn unescape_reverses_the_formatter() {
        assert_eq!(unescape(r"a\tb\nc\\d"), "a\tb\nc\\d");
    }
}
