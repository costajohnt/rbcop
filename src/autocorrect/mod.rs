//! Autocorrect (`-a` / `-A`): RuboCop's Team corrector collation (lib/rubocop/cop/team.rb) and the
//! Runner's inspect-correct-reinspect loop (lib/rubocop/runner.rb, `do_inspection_loop`).

pub mod corrector;
pub mod tree_rewriter;

use crate::config::value::Value;
use crate::config::{CopConfig, Resolved};
use crate::cop::Offense;
use crate::formatter::{line_col_at, line_starts};
use crate::runner::{self, FileReport};
use corrector::Corrector;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `-a`: only cops whose `Safe` and `SafeAutoCorrect` are true.
    Safe,
    /// `-A`: every cop.
    All,
}

/// RuboCop's registry order and `autocorrect_incompatible_with`, from scripts/autocorrect_registry.rb.
struct Registry {
    order: HashMap<&'static str, usize>,
    incompatible: HashMap<&'static str, Vec<&'static str>>,
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    let mut r = Registry { order: HashMap::new(), incompatible: HashMap::new() };
    for (i, line) in include_str!("registry.tsv").lines().enumerate() {
        let mut fields = line.split('\t');
        let name = fields.next().unwrap();
        r.order.insert(name, i);
        r.incompatible.insert(name, fields.collect());
    }
    r
});

/// A cop's line in registry.tsv (RuboCop's registration order, plugins after RuboCop's own cops).
pub fn registry_position(name: &str) -> Option<usize> {
    REGISTRY.order.get(name).copied()
}

/// A cop's position in RuboCop's registry (load) order: RuboCop's own cops, then each native
/// plugin's in the order the plugins were loaded (registry.tsv keeps the order within one plugin);
/// unknown names sort last.
pub fn registry_index(name: &str) -> (usize, usize) {
    let plugin = crate::helpers::cop_registry::plugin_of(name).map_or(0, |p| {
        crate::helpers::cop_registry::loaded_plugins().iter().position(|l| l.gem == p.gem).map_or(usize::MAX, |i| i + 1)
    });
    (plugin, REGISTRY.order.get(name).copied().unwrap_or(usize::MAX))
}

static DISABLE_UNCORRECTABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `--disable-uncorrectable`
pub fn set_disable_uncorrectable() {
    DISABLE_UNCORRECTABLE.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn disable_uncorrectable() -> bool {
    DISABLE_UNCORRECTABLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Cop::Base#autocorrect? (`correctable` is Cop.support_autocorrect?; `--disable-uncorrectable`
/// makes every cop correctable).
pub fn autocorrect(correctable: bool, cfg: &CopConfig, mode: Option<Mode>) -> bool {
    mode.is_some_and(|m| (correctable || disable_uncorrectable()) && cop_autocorrects(cfg, m))
}

/// AutocorrectLogic#skipped_unsafe_correction_with_disable_uncorrectable?
fn skipped_unsafe(cfg: &CopConfig, mode: Option<Mode>) -> bool {
    disable_uncorrectable()
        && mode == Some(Mode::Safe)
        && !(cfg.bool("Safe").unwrap_or(true) && cfg.bool("SafeAutoCorrect").unwrap_or(true))
}

/// Team#partition_by_correcting: the cop runs in the correcting pass (it autocorrects, or puts
/// todo comments where its unsafe correction is skipped).
pub fn correcting(correctable: bool, cfg: &CopConfig, mode: Option<Mode>) -> bool {
    autocorrect(correctable, cfg, mode) || skipped_unsafe(cfg, mode)
}

/// Whether the cop's uncorrected offenses get todo comments (`--disable-uncorrectable`).
pub fn todo(correctable: bool, cfg: &CopConfig, mode: Option<Mode>) -> bool {
    disable_uncorrectable() && correcting(correctable, cfg, mode)
}

/// AutocorrectLogic#disable_offense: `# rubocop:todo <cop>` at the end of the offense's first line,
/// or around its lines (`# rubocop:todo` before, `# rubocop:enable` after) when the comment would
/// make the line too long or land inside a multi-line literal.
pub fn disable_offense(
    c: &mut Corrector,
    src: &crate::source::Source,
    literals: &[(usize, usize)],
    config: &Resolved,
    cop: &str,
    start: usize,
    end: usize,
) {
    let eol = format!(" # rubocop:todo {cop}");
    // max_line_length
    let max = config
        .cop_enabled("Layout/LineLength")
        .then(|| config.for_cop("Layout/LineLength").num("Max").map_or(120.0, |n| n.f64()));
    let line_text = |off: usize| src.lines.get(src.line_of(off) - 1).copied().unwrap_or("");
    // line_with_eol_comment_too_long?
    let too_long = max.is_some_and(|m| (line_text(start).chars().count() + eol.chars().count()) as f64 > m);
    let line = src.line_of(start);
    let unbreakable = (start != end)
        .then(|| {
            literals.iter().copied().find(|&(ls, le)| {
                start < le && ls < end && (too_long || (line >= src.line_of(ls) && line < src.line_of(le)))
            })
        })
        .flatten();
    // range_by_lines: from the start of the first line to the end of the last (no newline).
    let by_lines = |(s, e): (usize, usize)| {
        let first = src.line_start(src.line_of(s));
        let last = src.line_of(e);
        (first, src.line_start(last) + src.lines.get(last - 1).map_or(0, |l| l.len()))
    };
    let around = match unbreakable {
        Some(r) => Some(by_lines(r)),
        None if too_long => Some(by_lines((start, end))),
        None => None,
    };
    match around {
        Some((s, e)) => {
            // disable_offense_before_and_after
            let first = line_text(s);
            let ws: String = first.chars().take_while(|ch| matches!(ch, ' ' | '\t' | '\x0b' | '\x0c' | '\r')).collect();
            c.wrap(s, e + 1, &format!("{ws}# rubocop:todo {cop}\n"), &format!("{ws}# rubocop:enable {cop}\n"));
        }
        None => {
            let eol_at = src.line_start(line) + line_text(start).len();
            c.insert_after(eol_at, eol_at, &eol);
        }
    }
}

/// Cop::AutocorrectLogic#autocorrect? for a cop that supports autocorrection (`Cop::correctable`).
pub fn cop_autocorrects(cfg: &CopConfig, mode: Mode) -> bool {
    // autocorrect_enabled?
    if matches!(cfg.get("AutoCorrect"), Some(Value::Bool(false)))
        || cfg.str("AutoCorrect") == Some("disabled")
        || (cfg.str("AutoCorrect") == Some("contextual") && crate::helpers::lsp::enabled())
    {
        return false;
    }
    mode == Mode::All || (cfg.bool("Safe").unwrap_or(true) && cfg.bool("SafeAutoCorrect").unwrap_or(true))
}

/// Team#collate_corrections: merge each cop's corrector in registry order, skipping a cop whose
/// corrections clobber what is already merged or that an earlier merged cop is incompatible with.
pub fn collate(mut by_cop: Vec<(&'static str, Corrector)>, source: Arc<str>) -> Option<Corrector> {
    by_cop.sort_by_key(|(name, _)| registry_index(name));
    let mut merged = Corrector::new(source);
    let mut skips: HashSet<&str> = HashSet::new();
    for (name, c) in &by_cop {
        if c.is_empty() || skips.contains(name) {
            continue;
        }
        let _ = merged.merge(c); // suppress_clobbering
        skips.extend(REGISTRY.incompatible.get(name).into_iter().flatten().copied());
    }
    (!merged.is_empty()).then_some(merged)
}

/// RuboCop's Runner::MAX_ITERATIONS.
const MAX_ITERATIONS: usize = 200;

/// The result of autocorrecting one file.
pub struct Corrected {
    pub report: FileReport,
    /// The new file content, if any correction was made.
    pub source: Option<String>,
    /// `Infinite loop detected in <path> and caused by <cops>`, if the loop gave up.
    pub error: Option<String>,
}

/// Runner#do_inspection_loop: inspect and correct until nothing changes. The report holds the
/// offenses corrected in earlier iterations (each with the source it was found in) plus all
/// offenses of the last iteration, deduplicated and sorted like RuboCop's.
pub fn correct(
    path: &Path,
    display: &str,
    text: String,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
) -> Corrected {
    crate::directives::take_warnings();
    runner::take_abort();
    let original: Arc<str> = Arc::from(text);
    let (mut offenses, mut current, changed, error, disabled) =
        inspection_loop(path, display, original.clone(), cfg, selected, mode);
    // Runner#finalize_corrections: each loop's result against the source it started from.
    let mut diffs = Vec::new();
    if changed {
        diffs.push((original, current.clone()));
    }
    let mut changed = changed;
    // Runner#add_redundant_disables (skipped when the loop raised): Lint/RedundantCopDisableDirective
    // judges the final source against every offense found, and when it removed directives the
    // whole loop runs once more; both offense lists are kept (`offenses |= new_offenses`).
    // (Nor when the file ends the run: Runner#check_for_redundant_disables? raised.)
    if error.is_none() && !runner::aborting() {
        let mut lines = Lines::default();
        let span = |o: &Offense| (o.cop, lines.line_col(o, o.start).0, lines.line_col(o, o.end.max(o.start)).0);
        let checked: Vec<runner::Span> = offenses.iter().chain(&disabled).map(span).collect();
        let normalised = normalise(&current);
        let (mut found, corrector) =
            runner::redundant_disables(path, display, &normalised, cfg, selected, mode, &checked);
        for o in &mut found {
            o.source = Some(normalised.clone());
        }
        offenses.extend(found);
        // Runner#file_offenses rejects the disabled offenses unless --display-suppressed.
        if runner::display_suppressed() {
            offenses.extend(disabled);
        }
        if let Some(c) = corrector {
            // RuboCop 1.91.0's redundant-disable team writes its correction to the file even under
            // `--diff` (its corrections are not deferred), so the loop it reruns diffs against
            // that source and the directives it removed are in no diff. rbcop writes nothing
            // (docs/known-gaps.md) but prints the same diffs.
            let rewritten: Arc<str> = Arc::from(c.rewrite());
            let (again, text, again_changed, err, again_disabled) =
                inspection_loop(path, display, rewritten.clone(), cfg, selected, mode);
            if again_changed {
                diffs.push((rewritten, text.clone()));
            }
            offenses.extend(again);
            if runner::display_suppressed() {
                offenses.extend(again_disabled);
            }
            current = text;
            changed = true;
            if let Some(e) = err {
                runner::file_error(e);
            }
        }
    }
    let report = FileReport {
        offenses: uniq_sorted(offenses),
        text: normalise(&current).to_string(),
        warnings: crate::directives::take_warnings(),
        abort: runner::take_abort(),
        diffs,
    };
    Corrected { source: changed.then(|| current.to_string()), report, error }
}

/// parser's Source::Buffer#source: CRLF normalised to LF (what cops see and corrections apply to).
fn normalise(text: &Arc<str>) -> Arc<str> {
    if text.contains("\r\n") { Arc::from(text.replace("\r\n", "\n")) } else { text.clone() }
}

/// Runner#do_inspection_loop: the offenses (unsorted, may repeat), the final source, whether it
/// changed, the infinite-loop error if any, and the last iteration's comment-disabled offenses.
/// Each iteration parses the source as written so far (RuboCop re-reads the file): its raw text
/// (CRLF intact, e.g. for Layout/EndOfLine) and the normalised text the corrections apply to, so
/// a CRLF a correction produced (`a\r` + inserted `\n`) disappears only if a later pass corrects.
fn inspection_loop(
    path: &Path,
    display: &str,
    mut current: Arc<str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
) -> (Vec<Offense>, Arc<str>, bool, Option<String>, Vec<Offense>) {
    let mut seen: Vec<Arc<str>> = Vec::new();
    let mut disabled = Vec::new();
    let mut by_iteration: Vec<Vec<Offense>> = Vec::new();
    let mut changed = false;
    let mut error = None;
    loop {
        // check_for_infinite_loop, then the iteration limit.
        if let Some(start) = seen.iter().position(|s| *s == current) {
            error = Some(infinite_loop(path, &by_iteration[start..]));
            break;
        }
        seen.push(current.clone());
        if seen.len() > MAX_ITERATIONS {
            error = Some(infinite_loop(path, &by_iteration[by_iteration.len() - 1..]));
            break;
        }
        if let Some(last) = by_iteration.last_mut() {
            last.retain(|o| o.corrected);
        }
        let normalised = normalise(&current);
        let raw = (normalised.len() != current.len()).then_some(&*current);
        let (mut offenses, corrector, off) =
            runner::analyze_for_correction(path, display, &normalised, raw, cfg, selected, mode);
        disabled = off;
        for o in offenses.iter_mut().chain(disabled.iter_mut()) {
            o.source = Some(normalised.clone());
        }
        by_iteration.push(offenses);
        match corrector {
            Some(c) => {
                current = Arc::from(c.rewrite());
                changed = true;
            }
            None => break,
        }
    }
    (by_iteration.concat(), current, changed, error, disabled)
}

/// Autocorrect one file on disk: read, inspect and correct. The caller writes `source` back with
/// `write_source` (in file order, so a failure stops the run before later files change). Err is a
/// read failure, worded as RuboCop's.
pub fn correct_file(
    path: &Path,
    display: &str,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
) -> Result<Corrected, String> {
    let bytes = std::fs::read(path).map_err(|e| runner::io_error(&e, path))?;
    Ok(correct_bytes(path, display, bytes, cfg, selected, mode))
}

/// `correct` for a source as read (a file or stdin).
pub fn correct_bytes(
    path: &Path,
    display: &str,
    bytes: Vec<u8>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
) -> Corrected {
    match String::from_utf8(bytes) {
        Ok(text) if crate::source::encoding_error(text.as_bytes()).is_none() => {
            correct(path, display, text, cfg, selected, mode)
        }
        Ok(text) => {
            let report = runner::inspect_bytes(path, display, text.into_bytes(), cfg, selected);
            Corrected { report, source: None, error: None }
        }
        // ponytail: never rewrite a source that is not valid UTF-8; it is only inspected.
        Err(e) => {
            let report = runner::inspect_bytes(path, display, e.into_bytes(), cfg, selected);
            Corrected { report, source: None, error: None }
        }
    }
}

/// RuboCop's `File.open(path, 'w') { |f| f.write(new) }` (symlinks followed, the file's mode kept,
/// a file it may not write refused), except that a crash or a full disk never leaves the file
/// truncated: the content goes to a temporary file in the same directory, which is renamed over
/// the target. A rename would change more than the content of a hard-linked file or of one owned
/// by someone else, and cannot be done in a directory rbcop may not write; those are written in place.
///
/// Deliberate difference: a file whose canonical path is not under one of `roots` (canonical
/// paths, see `outside_roots`) is refused with PermissionDenied, checked here right before the
/// write. The directories from that root down to the file are then opened one by one without
/// following symlinks, and every later step works relative to the last one (`openat`, `renameat`),
/// so a directory swapped for a symlink after the check fails the write instead of redirecting it.
// ponytail: extended attributes and ACLs are not copied to the renamed file.
pub fn write_source(path: &Path, new: &str, roots: &[PathBuf]) -> std::io::Result<()> {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let target = fs::canonicalize(path)?;
    let Some(root) = roots.iter().find(|r| target.starts_with(r)) else {
        let msg = format!("{} resolves to {}, outside the project", path.display(), target.display());
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg));
    };
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(std::io::ErrorKind::InvalidInput.into());
    };
    // A file argument is its own root: start at its directory.
    let base = if *root == target { parent } else { root.as_path() };
    // Search-only descriptors: like path-based `open`/`rename`, the walk needs only execute
    // permission on each directory, not read.
    let mut dir = at::open_dir(base)?;
    for c in parent.strip_prefix(base).unwrap_or(Path::new("")).components() {
        dir = at::open(&dir, c.as_os_str(), at::DIR | libc::O_NOFOLLOW, 0)?;
    }
    // File.open(path, 'w') fails on a file that is not writable; the rename must not get round that.
    let mut file = at::open(&dir, name, libc::O_WRONLY | libc::O_NOFOLLOW, 0)?;
    let meta = file.metadata()?;
    // SAFETY: geteuid has no preconditions.
    if meta.nlink() == 1 && meta.uid() == unsafe { libc::geteuid() } {
        // An unguessable name, created exclusively (O_EXCL: never an existing file or symlink) and
        // readable by the owner only until it has the file's own group and mode, before any content.
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let tmp = format!(".{}.rbcop-{}-{n}-{nanos:x}.tmp", name.to_string_lossy(), std::process::id());
        let tmp = std::ffi::OsStr::new(&tmp);
        let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW;
        if let Ok(mut f) = at::open(&dir, tmp, flags, 0o600) {
            // The file's group, too (a group rbcop's user is not in cannot be given: write in place).
            if std::os::unix::fs::fchown(&f, None, Some(meta.gid())).is_ok() {
                let written = (|| {
                    f.set_permissions(meta.permissions())?;
                    f.write_all(new.as_bytes())?;
                    f.sync_all()?;
                    at::rename(&dir, tmp, name)
                })();
                if written.is_err() {
                    let _ = at::unlink(&dir, tmp);
                }
                return written;
            }
            drop(f);
            let _ = at::unlink(&dir, tmp);
        }
    }
    file.set_len(0)?;
    file.write_all(new.as_bytes())
}

/// Create or replace (`append`: append to) `path` with `content`, refusing a file whose directory
/// resolves outside the `roots` (write_roots), a symlink at any step below the root, and an
/// existing file that is not a regular file of the current user with a single link (a hard link
/// to a file elsewhere). A replaced file is written to a new file renamed over it. Returns the
/// open file.
pub fn write_contained(path: &Path, content: &[u8], roots: &[PathBuf], append: bool) -> std::io::Result<std::fs::File> {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    let refuse = |what: String| std::io::Error::new(std::io::ErrorKind::PermissionDenied, what);
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let Some(name) = path.file_name() else { return Err(std::io::ErrorKind::InvalidInput.into()) };
    let dir_path = std::fs::canonicalize(parent)?;
    let Some(root) = roots.iter().find(|r| dir_path.starts_with(r)) else {
        return Err(refuse(format!("{} resolves to {}, outside the project", path.display(), dir_path.display())));
    };
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(refuse(format!("{} is a symbolic link", path.display())));
    }
    let mut dir = at::open_dir(root)?;
    for c in dir_path.strip_prefix(root).unwrap_or(Path::new("")).components() {
        dir = at::open(&dir, c.as_os_str(), at::DIR | libc::O_NOFOLLOW, 0)?;
    }
    let create = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW;
    // Opened without truncating (O_NONBLOCK: a FIFO does not hang the open) and checked first.
    let existing = match at::open(&dir, name, libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK, 0) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut f = at::open(&dir, name, create, 0o666)?;
            f.write_all(content)?;
            return Ok(f);
        }
        Err(e) => return Err(e),
    };
    let meta = existing.metadata()?;
    // SAFETY: geteuid has no preconditions.
    let mine = meta.uid() == unsafe { libc::geteuid() };
    if !meta.file_type().is_file() || meta.nlink() != 1 || !mine {
        return Err(refuse(format!("{} is not a regular file of its own (a link, or another user's)", path.display())));
    }
    if append {
        // The descriptor just checked, switched to appending (no second open by name).
        let mut f = existing;
        at::set_append(&f)?;
        f.write_all(content)?;
        return Ok(f);
    }
    // A new file renamed over the name: whatever the name now points at is replaced, not written.
    drop(existing);
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let tmp = format!(".{}.rbcop-{}-{n}-{nanos:x}.tmp", name.to_string_lossy(), std::process::id());
    let tmp = std::ffi::OsStr::new(&tmp);
    let mut f = at::open(&dir, tmp, create, 0o600)?;
    let written = (|| {
        f.set_permissions(meta.permissions())?;
        f.write_all(content)?;
        at::rename(&dir, tmp, name)
    })();
    if let Err(e) = written {
        let _ = at::unlink(&dir, tmp);
        return Err(e);
    }
    Ok(f)
}

/// `openat`, `renameat` and `unlinkat` relative to an open directory.
mod at {
    use std::ffi::{CString, OsStr};
    use std::fs::File;
    use std::io::{Error, Result};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    fn c(name: &OsStr) -> Result<CString> {
        Ok(CString::new(name.as_bytes())?)
    }

    /// Directory flags for a descriptor used only as the base of `*at` calls.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub const DIR: libc::c_int = libc::O_PATH | libc::O_DIRECTORY;
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    pub const DIR: libc::c_int = libc::O_SEARCH | libc::O_DIRECTORY;
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd"
    )))]
    pub const DIR: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY;

    pub fn open_dir(path: &std::path::Path) -> Result<File> {
        let path = c(path.as_os_str())?;
        // SAFETY: a NUL-terminated string alive for the call.
        let fd = check(unsafe { libc::open(path.as_ptr(), DIR | libc::O_CLOEXEC) })?;
        // SAFETY: `fd` was just returned by open and is owned by nothing else.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn check(r: libc::c_int) -> Result<libc::c_int> {
        if r < 0 { Err(Error::last_os_error()) } else { Ok(r) }
    }

    pub fn open(dir: &File, name: &OsStr, flags: libc::c_int, mode: libc::mode_t) -> Result<File> {
        let name = c(name)?;
        // SAFETY: `dir` is an open descriptor and `name` a NUL-terminated string, both alive for
        // the call; the mode is passed promoted to c_uint, as the variadic argument expects.
        let fd = check(unsafe {
            libc::openat(dir.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC, libc::c_uint::from(mode))
        })?;
        // SAFETY: `fd` was just returned by openat and is owned by nothing else.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    /// `fcntl(F_SETFL)`: O_APPEND on, O_NONBLOCK off.
    pub fn set_append(f: &File) -> Result<()> {
        let fd = f.as_raw_fd();
        // SAFETY: fcntl on an open descriptor owned by `f`.
        let flags = check(unsafe { libc::fcntl(fd, libc::F_GETFL) })?;
        // SAFETY: as above.
        check(unsafe { libc::fcntl(fd, libc::F_SETFL, (flags | libc::O_APPEND) & !libc::O_NONBLOCK) }).map(drop)
    }

    pub fn rename(dir: &File, from: &OsStr, to: &OsStr) -> Result<()> {
        let (from, to) = (c(from)?, c(to)?);
        let d = dir.as_raw_fd();
        // SAFETY: an open descriptor and two NUL-terminated strings, all alive for the call.
        check(unsafe { libc::renameat(d, from.as_ptr(), d, to.as_ptr()) }).map(drop)
    }

    pub fn unlink(dir: &File, name: &OsStr) -> Result<()> {
        let name = c(name)?;
        // SAFETY: an open descriptor and a NUL-terminated string, both alive for the call.
        check(unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) }).map(drop)
    }
}

/// Whether the canonical path `target` lies outside every root: canonical directories (the
/// working directory, directory arguments) and canonical files (file arguments, matched exactly).
pub fn outside_roots(target: &Path, roots: &[PathBuf]) -> bool {
    !roots.iter().any(|r| target.starts_with(r))
}

/// `write_source`'s roots for a run: the working directory and each command-line path, canonical.
pub fn write_roots(cwd: &Path, args: &[PathBuf]) -> Vec<PathBuf> {
    std::iter::once(cwd.to_path_buf())
        .chain(args.iter().map(|a| crate::config::expand_path(a, cwd)))
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect()
}

fn infinite_loop(path: &Path, iterations: &[Vec<Offense>]) -> String {
    let cause: Vec<String> = iterations
        .iter()
        .map(|offenses| {
            // InfiniteCorrectionLoop maps the team's offenses to cop names: the team reports per
            // cop, and Registry#sort! orders its cops by name.
            let mut names: Vec<&str> = offenses.iter().map(|o| o.cop).collect();
            names.sort_unstable();
            names.dedup();
            names.join(", ")
        })
        .collect();
    format!("Infinite loop detected in {} and caused by {}", path.display(), cause.join(" -> "))
}

/// Line/column lookups for offenses of a file's sources (one per -A iteration), with each
/// source's line starts indexed once: rescanning the source per offense made -A quadratic in the
/// number of corrections.
#[derive(Default)]
struct Lines(HashMap<usize, Vec<usize>>);

impl Lines {
    fn line_col(&mut self, o: &Offense, off: usize) -> (usize, usize) {
        let text = o.source.as_deref().unwrap_or("");
        let starts = self.0.entry(text.as_ptr() as usize).or_insert_with(|| line_starts(text));
        line_col_at(text, starts, off)
    }
}

/// `offenses.flatten.uniq` then `sort`: Offense equality and order are by
/// (line, column, cop name, message, severity).
fn uniq_sorted(offenses: Vec<Offense>) -> Vec<Offense> {
    let mut lines = Lines::default();
    let mut key = |o: &Offense| {
        let (line, col) = lines.line_col(o, o.start);
        (line, col, o.cop, o.message.clone(), o.severity)
    };
    let mut seen = HashSet::new();
    let mut out: Vec<(_, Offense)> =
        offenses.into_iter().map(|o| (key(&o), o)).filter(|(k, _)| seen.insert(k.clone())).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.into_iter().map(|(_, o)| o).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(src: &Arc<str>, edit: impl FnOnce(&mut Corrector)) -> Corrector {
        let mut c = Corrector::new(src.clone());
        edit(&mut c);
        c
    }

    #[test]
    fn collate_follows_registry_order_clobbering_and_incompatibility() {
        let src: Arc<str> = Arc::from("abcdef");
        // Registry order, not argument order: Layout/TrailingWhitespace merges before
        // Style/NilComparison, so the clobbering NilComparison edit is the one dropped.
        let by_cop = vec![
            ("Style/NilComparison", fix(&src, |c| c.replace(1, 2, "Y"))),
            ("Layout/TrailingWhitespace", fix(&src, |c| c.replace(1, 2, "X"))),
            ("Layout/IndentationStyle", fix(&src, |c| c.remove(4, 5))),
        ];
        assert_eq!(collate(by_cop, src.clone()).unwrap().rewrite(), "aXcdf");
        // Layout/EmptyLineBetweenDefs is autocorrect_incompatible_with Layout/EmptyLines.
        let by_cop = vec![
            ("Layout/EmptyLines", fix(&src, |c| c.remove(0, 1))),
            ("Layout/EmptyLineBetweenDefs", fix(&src, |c| c.remove(5, 6))),
        ];
        assert_eq!(collate(by_cop, src.clone()).unwrap().rewrite(), "abcde");
        assert!(collate(vec![("Layout/EmptyLines", fix(&src, |_| {}))], src).is_none());
    }

    #[test]
    fn write_source_keeps_mode_and_refuses_outside_roots() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rbcop-write-source-{}", std::process::id()));
        let sub = dir.join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("x.rb");
        std::fs::write(&file, "old\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        let root = std::fs::canonicalize(&dir).unwrap();
        write_source(&file, "new\n", std::slice::from_ref(&root)).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new\n");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o640);
        assert_eq!(std::fs::read_dir(&sub).unwrap().count(), 1, "temp file left behind");
        // A file argument is its own root.
        write_source(&file, "again\n", &[std::fs::canonicalize(&file).unwrap()]).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "again\n");
        let err = write_source(&file, "no\n", &[root.join("elsewhere")]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
