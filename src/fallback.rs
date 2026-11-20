//! Cops rbcop cannot run (plugin cops it does not implement, cops from `require:`d Ruby files):
//! a RuboCop subprocess that runs exactly those cops, whose offenses are merged into rbcop's report,
//! when a RuboCop is found (`rubocop`); else, or under RBCOP_FALLBACK=0, a notice on stderr.

use rbcop::autocorrect::Mode;
use rbcop::config::{Options, Resolved};
use rbcop::cop::{Offense, Severity};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

const FORMATTER: &str = include_str!("fallback_formatter.rb");

/// Loaded through RUBYOPT before RuboCop: the child reads no `.rubocop` arguments file (the prepend
/// on the singleton class outlives RuboCop's own `def self.read_as_arguments`).
const PRELOAD: &str = "module RuboCop\n  class ArgumentsFile\n    singleton_class.prepend(Module.new { def read_as_arguments = [] })\n  end\nend\n";

/// The RuboCop command the fallback runs, or None when it does not run. RBCOP_FALLBACK=0 turns it
/// off; otherwise `RBCOP_RUBOCOP` when set, else `bundle exec rubocop` when the bundle locks the
/// rubocop gem, else `rubocop` when it is on PATH. RBCOP_FALLBACK=1 runs `rubocop` even when none
/// was found (and fails if it cannot start). Called only once something needs RuboCop, so a project
/// that needs nothing never looks. In an editor or server context (`editor_context`) only
/// RBCOP_FALLBACK=1 runs it. Never under --no-ruby, whatever RBCOP_FALLBACK says.
pub fn rubocop() -> Option<String> {
    if rbcop::no_ruby::on() {
        return None;
    }
    let mode = std::env::var("RBCOP_FALLBACK").unwrap_or_default();
    if mode == "0" || (mode != "1" && EDITOR.load(Relaxed)) {
        return None;
    }
    if let Some(cmd) = std::env::var("RBCOP_RUBOCOP").ok().filter(|v| !v.trim().is_empty()) {
        return Some(cmd);
    }
    if bundle_has_rubocop() && on_path("bundle") {
        return Some("bundle exec rubocop".into());
    }
    (mode == "1" || on_path("rubocop")).then(|| "rubocop".into())
}

static EDITOR: AtomicBool = AtomicBool::new(false);
static ANNOUNCED: AtomicBool = AtomicBool::new(false);

/// The run comes from an editor or a server (`editor_context` in src/main.rs): no RuboCop unless
/// RBCOP_FALLBACK=1.
pub fn editor_context() {
    EDITOR.store(true, Relaxed);
}

/// Once per run, unless RBCOP_FALLBACK=1 asked for it: which RuboCop runs the project's Ruby.
pub fn announce(program: &str, what: &str) {
    if std::env::var_os("RBCOP_FALLBACK").is_none_or(|v| v != "1") && !ANNOUNCED.swap(true, Relaxed) {
        eprintln!("rbcop: running {program} for {what}; set RBCOP_FALLBACK=0 to skip RuboCop");
    }
}

/// The notice's second line when no RuboCop was found; None under RBCOP_FALLBACK=0, --no-ruby or
/// in an editor.
pub fn missing() -> Option<&'static str> {
    (std::env::var_os("RBCOP_FALLBACK").is_none_or(|v| v != "0") && !EDITOR.load(Relaxed) && !rbcop::no_ruby::on())
        .then_some(
            "rbcop: RuboCop runs them when one is found: add the rubocop gem to the bundle, put rubocop on PATH, or \
         set RBCOP_RUBOCOP",
        )
}

/// Bundler's Gemfile lookup (BUNDLE_GEMFILE, else Gemfile or gems.rb in the working directory or a
/// parent), and whether its lockfile has the rubocop gem among its specs.
fn bundle_has_rubocop() -> bool {
    let gemfile = match std::env::var_os("BUNDLE_GEMFILE").filter(|v| !v.is_empty()) {
        Some(g) => Some(PathBuf::from(g)),
        None => std::env::current_dir()
            .ok()
            .and_then(|cwd| cwd.ancestors().flat_map(|d| [d.join("Gemfile"), d.join("gems.rb")]).find(|p| p.is_file())),
    };
    let Some(gemfile) = gemfile else { return false };
    let lock = if gemfile.file_name().is_some_and(|n| n == "gems.rb") {
        gemfile.with_file_name("gems.locked")
    } else {
        PathBuf::from(format!("{}.lock", gemfile.display()))
    };
    std::fs::read_to_string(lock).is_ok_and(|t| t.lines().any(|l| l.starts_with("    rubocop (")))
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| {
            let f = d.join(name);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                f.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            f.is_file()
        })
    })
}

/// Hands the whole run to RuboCop, for Ruby code rbcop cannot load (`-r`/`--require`, `--plugin`,
/// a custom formatter class): the same arguments, standard streams and exit status.
pub fn delegate(program: &str, args: &[String]) -> u8 {
    let mut words = program.split_whitespace();
    let Some(bin) = words.next() else { return 2 };
    let mut cmd = Command::new(bin);
    cmd.args(words).args(args);
    #[cfg(unix)]
    let e = {
        use std::os::unix::process::CommandExt;
        cmd.exec()
    };
    #[cfg(not(unix))]
    let e = match cmd.status() {
        Ok(s) => return s.code().map_or(2, |c| c as u8),
        Err(e) => e,
    };
    eprintln!("rbcop: cannot run {program}: {e}");
    2
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
pub fn notice(skipped: &BTreeSet<String>, required_code: bool, copless_plugins: &[&str]) -> String {
    let hint = match rbcop::no_ruby::on() {
        true => "--no-ruby keeps RuboCop from running them",
        false => "set RBCOP_FALLBACK=1 to run them with RuboCop",
    };
    format!("rbcop: not running {}; {hint}", what(skipped, required_code, copless_plugins))
}

/// `3 cops it does not implement (Rails: 2, RSpec: 1) or cops from Ruby code the config requires`
pub fn what(skipped: &BTreeSet<String>, required_code: bool, copless_plugins: &[&str]) -> String {
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
    if !copless_plugins.is_empty() {
        let s = if copless_plugins.len() == 1 { "" } else { "s" };
        parts.push(format!("the Ruby code of plugin{s} {}", copless_plugins.join(", ")));
    }
    parts.join(" or ")
}

/// What a child run reports.
#[derive(Default)]
pub struct Found {
    /// Its offenses by file.
    pub offenses: HashMap<PathBuf, Vec<Offense>>,
    /// The files a cop raised an error or a warning on (Runner#save_in_cache keeps nothing then).
    pub uncached: HashSet<PathBuf>,
    /// The digest of the Ruby files it had loaded before inspecting any file (`Cached`).
    pub loaded: Option<String>,
    /// A cop it may run declares an external dependency (Base#external_dependency_checksum),
    /// which ResultCache hashes and rbcop cannot.
    pub external_dependency: bool,
    /// With `--stdin` and -a/-A: the source as RuboCop corrected it.
    pub stdin: Option<Vec<u8>>,
}

/// With rbcop's result cache, the child tells it the Ruby files it has loaded, before inspecting
/// any file: called with their digest (each path and its contents, as ResultCache.source_checksum
/// reads them), it returns the files whose offenses rbcop has cached, which the child skips.
pub type Cached<'a> = &'a dyn Fn(&str) -> HashSet<PathBuf>;

/// The digest of the Ruby files listed in `list` (one per line): each path with its contents (a
/// feature without a readable file, as such).
fn loaded_digest(list: &str) -> String {
    let mut h = xxhash_rust::xxh3::Xxh3::new();
    for f in list.lines() {
        h.update(f.as_bytes());
        match std::fs::read(f) {
            Ok(b) => {
                h.update(&(b.len() as u64).to_le_bytes());
                h.update(&b);
            }
            Err(_) => h.update(b"\0missing"),
        }
        h.update(b"\0");
    }
    format!("{:032x}", h.digest128())
}

/// The line the child writes to stderr once `loaded` is in place (the formatter's `started`).
const LOADED: &[u8] = b"\x1Erbcop-fallback-loaded\n";

/// What the RuboCop subprocess needs from the command line.
#[derive(Clone, Copy)]
pub struct Child<'a> {
    /// The RuboCop command (`rubocop()`).
    pub program: &'a str,
    /// What it runs, for `announce` (`what()`).
    pub what: &'a str,
    pub opts: &'a Options,
    pub except: &'a [String],
    pub only_guide: bool,
    pub force_exclusion: bool,
    pub autocorrect: Option<Mode>,
    /// The `--stdin` source; `files` is then its one path.
    pub stdin: Option<&'a [u8]>,
}

/// Runs RuboCop (`c.program`) on `files` with every cop rbcop implements excluded (the child
/// itself skips the files none of its cops may inspect, and those `cached` names), and returns
/// what it reports (`Found`). With `implemented` empty RuboCop runs every cop,
/// Lint/Syntax included: the files rbcop leaves wholly to it (Store::copless_plugin_file). Under -a/-A RuboCop corrects (and writes) first, so
/// rbcop then inspects the result; its offenses refer to the source before correction.
pub fn run(
    files: &[PathBuf],
    cwd: &Path,
    implemented: &[&str],
    c: Child,
    cached: Option<Cached>,
) -> Result<Found, String> {
    let mut out = HashMap::new();
    let mut found = Found::default();
    if files.is_empty() {
        return Ok(found);
    }
    let program = c.program;
    announce(program, c.what);
    let mut words = program.split_whitespace();
    let Some(bin) = words.next() else { return Err("rbcop: RBCOP_RUBOCOP is empty".into()) };
    // A fresh directory (create_dir fails if it exists) so nothing else in the temp dir can choose
    // the Ruby that runs.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("rbcop-fallback-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| format!("rbcop: RBCOP_FALLBACK: {}: {e}", dir.display()))?;
    struct Remove(PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let dir = Remove(dir);
    let write = |name: &str, text: &str| -> Result<PathBuf, String> {
        let path = dir.0.join(name);
        std::fs::write(&path, text).map_err(|e| format!("rbcop: RBCOP_FALLBACK: {}: {e}", path.display()))?;
        Ok(path)
    };
    let fmt = write("formatter.rb", FORMATTER)?;
    let pre = write("preload.rb", PRELOAD)?.display().to_string();
    // RUBYOPT is split on whitespace, with no quoting.
    if pre.contains(char::is_whitespace) {
        return Err(format!("rbcop: RBCOP_FALLBACK: the temp directory's path has whitespace: {pre}"));
    }
    let rubyopt = match std::env::var("RUBYOPT") {
        Ok(v) if !v.is_empty() => format!("-r{pre} {v}"),
        _ => format!("-r{pre}"),
    };

    // Lint/Syntax cannot be excluded; the formatter drops its offenses.
    let except: Vec<&str> = implemented
        .iter()
        .copied()
        .filter(|&n| n != "Lint/Syntax")
        .chain(c.except.iter().map(String::as_str))
        .collect();
    let mut cmd = Command::new(bin);
    // The offenses go to a file: what cops print themselves (rubocop-require_tools' notes) reaches
    // stdout as it would from RuboCop.
    let report = dir.0.join("offenses");
    cmd.args(words).args(["--cache", "false", "--require"]).arg(&fmt).args(["--format", "RbcopFallbackFormatter"]);
    cmd.arg("--out").arg(&report);
    if !except.is_empty() {
        cmd.args(["--except", &except.join(",")]);
    }
    if implemented.is_empty() {
        cmd.env("RBCOP_FALLBACK_SYNTAX", "1");
    }
    cmd.current_dir(cwd)
        .env_remove("RBCOP_FALLBACK")
        // rbcop has applied `.rubocop` and RUBOCOP_OPTS and forwards what the child needs. Read
        // again, a `--format`, `-o` or `--stderr` there would break the output protocol.
        .env_remove("RUBOCOP_OPTS")
        .env("RUBYOPT", rubyopt)
        .env("RBCOP_REGISTRY", rbcop::helpers::cop_registry::REGISTRY.join(","))
        .stdin(Stdio::null());
    // The handshake's answer goes to the child's fd 3, so its stdin stays RuboCop's.
    let answer = match cached {
        Some(_) => {
            let (read, write) = std::io::pipe().map_err(|e| format!("rbcop: RBCOP_FALLBACK: {e}"))?;
            let fd = std::os::fd::AsRawFd::as_raw_fd(&read);
            // SAFETY: dup2 and fcntl are async-signal-safe. dup2 leaves an fd that already is 3
            // as it is (close-on-exec, as std::io::pipe makes it), so that one is cleared instead.
            unsafe {
                std::os::unix::process::CommandExt::pre_exec(&mut cmd, move || {
                    let r = if fd == 3 { libc::fcntl(3, libc::F_SETFD, 0) } else { libc::dup2(fd, 3) };
                    if r == -1 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
                });
            }
            cmd.env("RBCOP_FALLBACK_HANDSHAKE", &dir.0);
            Some((read, write))
        }
        None => None,
    };
    let o = c.opts;
    if let Some(cfg) = &o.config_file {
        cmd.arg("--config").arg(cfg);
    }
    if !o.only.is_empty() {
        cmd.args(["--only", &o.only.join(",")]);
    }
    // The pending flags are forwarded only as given: --disable-pending-cops wins over `NewCops:
    // enable` (Registry#enabled_pending_cop?). The child's pending-cops banner is silenced by the
    // formatter file; rbcop prints its own.
    for (on, flag) in [
        (o.enable_pending_cops, "--enable-pending-cops"),
        (o.disable_pending_cops, "--disable-pending-cops"),
        (o.force_default_config, "--force-default-config"),
        (rbcop::directives::ignoring_disable_comments(), "--ignore-disable-comments"),
        (rbcop::runner::display_suppressed(), "--display-suppressed"),
        (rbcop::helpers::lsp::enabled(), "--editor-mode"),
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
    // With --stdin, RuboCop reads the same source on its stdin; under -a/-A the formatter file
    // writes the corrected source to `stdin` rather than printing it.
    let stdin_out = dir.0.join("stdin");
    match c.stdin {
        Some(_) => {
            cmd.arg("--stdin").args(files).stdin(Stdio::piped()).env("RBCOP_FALLBACK_STDIN", &stdin_out);
        }
        None => {
            cmd.arg("--").args(files);
        }
    }
    // Under -a/-A the offsets refer to the source RuboCop read, which it then rewrites.
    let originals: HashMap<&PathBuf, Arc<str>> = if c.autocorrect.is_some() {
        files
            .iter()
            .filter_map(|p| {
                let text = match c.stdin {
                    Some(b) => String::from_utf8_lossy(b).into_owned(),
                    None => std::fs::read_to_string(p).ok()?,
                };
                Some((p, Arc::from(text.replace("\r\n", "\n"))))
            })
            .collect()
    } else {
        HashMap::new()
    };
    // Its stderr is passed on once it exits, without the handshake's line.
    let mut child = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("rbcop: RBCOP_FALLBACK: cannot run {program}: {e}"))?;
    // Written from a thread, so a child that reads it late (or never) does not block rbcop.
    let feeder = child.stdin.take().zip(c.stdin.map(<[u8]>::to_vec)).map(|(mut pipe, text)| {
        std::thread::spawn(move || {
            let _ = std::io::Write::write_all(&mut pipe, &text);
        })
    });
    let mut answer = answer.map(|(read, write)| {
        drop(read);
        write
    });
    let mut pipe = child.stderr.take().expect("piped");
    let (ready, at_loaded) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut err = Vec::new();
        let mut buf = [0; 1 << 14];
        let mut told = false;
        while let Ok(n @ 1..) = pipe.read(&mut buf) {
            err.extend_from_slice(&buf[..n]);
            if !told && let Some(at) = err.windows(LOADED.len()).position(|w| w == LOADED) {
                err.drain(at..at + LOADED.len());
                told = true;
                let _ = ready.send(());
            }
        }
        err
    });
    // The child stops after `loaded` until it reads the cached files (to the end of fd 3); a
    // child that ends before (a config error) never says it is ready.
    if let Some(cached) = cached
        && at_loaded.recv().is_ok()
        && let Ok(list) = std::fs::read_to_string(dir.0.join("loaded"))
    {
        let loaded = loaded_digest(&list);
        let skip = cached(&loaded);
        found.loaded = Some(loaded);
        if let Some(mut write) = answer.take() {
            let mut text = String::new();
            for p in &skip {
                text.push_str(&format!("{}\n", p.display()));
            }
            let _ = std::io::Write::write_all(&mut write, text.as_bytes());
        }
    }
    drop(answer);
    let status = child.wait();
    if let Some(feeder) = feeder {
        let _ = feeder.join();
    }
    let err = reader.join().unwrap_or_default();
    let _ = std::io::Write::write_all(&mut std::io::stderr(), &err);
    let report = std::fs::read(&report).unwrap_or_default();
    found.stdin = std::fs::read(&stdin_out).ok();
    drop(dir);
    let status = status.map_err(|e| format!("rbcop: RBCOP_FALLBACK: cannot run {program}: {e}"))?;
    // 0: no offenses, 1: offenses; anything else is RuboCop failing (its stderr was passed through).
    // The formatter's last line tells a run from a wrapper failing with 1 (an rbenv shim with the
    // project's Ruby missing, `bundle exec` outside an installed bundle).
    let report = String::from_utf8_lossy(&report);
    let body = report.strip_suffix("rbcop-fallback-done\n");
    let (true, Some(body)) = (matches!(status.code(), Some(0 | 1)), body) else {
        return Err(format!(
            "rbcop: RBCOP_FALLBACK: {program} failed ({status}); set RBCOP_FALLBACK=0 to skip the cops rbcop cannot run"
        ));
    };
    let mut names: HashMap<String, &'static str> = HashMap::new();
    for line in body.lines() {
        if let Some(path) = line.strip_prefix("rbcop-fallback-uncached\t") {
            found.uncached.insert(PathBuf::from(unescape(path)));
            continue;
        }
        if line == "rbcop-fallback-external-dependency" {
            found.external_dependency = true;
            continue;
        }
        let mut f: Vec<&str> = line.split('\t').collect();
        // The corrections field is optional (a stand-in child may print the eleven before it).
        if f.len() == 11 {
            f.push("");
        }
        let [path, start, end, severity, correctable, corrected, global, cop, message, disabled, justification, edits] =
            f[..]
        else {
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
            disabled: disabled == "1",
            justification: (!justification.is_empty()).then(|| unescape(justification)),
            todo: false,
            edits: (!edits.is_empty()).then(|| edits.split(';').filter_map(parse_edit).collect()),
        };
        out.entry(path).or_insert_with(Vec::new).push(offense);
    }
    found.offenses = out;
    Ok(found)
}

/// One `begin,end,hex(replacement)` correction of the formatter's last field.
pub fn parse_edit(s: &str) -> Option<(usize, usize, String)> {
    let mut f = s.split(',');
    let (b, e, hex) = (f.next()?.parse().ok()?, f.next()?.parse().ok()?, f.next()?.as_bytes());
    let bytes: Option<Vec<u8>> =
        hex.chunks(2).map(|c| std::str::from_utf8(c).ok().and_then(|c| u8::from_str_radix(c, 16).ok())).collect();
    Some((b, e, String::from_utf8_lossy(&bytes?).into_owned()))
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
            notice(&cops, true, &[]),
            "rbcop: not running 3 cops it does not implement (RSpec: 1, Rails: 2) or cops from Ruby code the \
             config requires; set RBCOP_FALLBACK=1 to run them with RuboCop"
        );
        assert_eq!(
            notice(&BTreeSet::new(), true, &[]),
            "rbcop: not running cops from Ruby code the config requires; set RBCOP_FALLBACK=1 to run them with RuboCop"
        );
        assert_eq!(
            notice(&BTreeSet::new(), false, &["rubocop-erb"]),
            "rbcop: not running the Ruby code of plugin rubocop-erb; set RBCOP_FALLBACK=1 to run them with RuboCop"
        );
    }

    #[test]
    fn unescape_reverses_the_formatter() {
        assert_eq!(unescape(r"a\tb\nc\\d"), "a\tb\nc\\d");
        assert_eq!(parse_edit("0,3,0a78"), Some((0, 3, "\nx".to_string())));
        assert_eq!(parse_edit("4,4,"), Some((4, 4, String::new())));
    }
}
