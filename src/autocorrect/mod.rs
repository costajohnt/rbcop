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

/// A cop's position in RuboCop's registry (load) order; unknown names sort last.
pub fn registry_index(name: &str) -> usize {
    REGISTRY.order.get(name).copied().unwrap_or(usize::MAX)
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
    let (mut offenses, mut current, changed, error, disabled) =
        inspection_loop(path, display, Arc::from(text), cfg, selected, mode);
    let mut changed = changed;
    // Runner#add_redundant_disables (skipped when the loop raised): Lint/RedundantCopDisableDirective
    // judges the final source against every offense found, and when it removed directives the
    // whole loop runs once more; both offense lists are kept (`offenses |= new_offenses`).
    if error.is_none() {
        let mut lines = Lines::default();
        let span = |o: &Offense| (o.cop, lines.line_col(o, o.start).0, lines.line_col(o, o.end.max(o.start)).0);
        let checked: Vec<runner::Span> = offenses.iter().map(span).chain(disabled).collect();
        let normalised = normalise(&current);
        let (mut found, corrector) =
            runner::redundant_disables(path, display, &normalised, cfg, selected, mode, &checked);
        for o in &mut found {
            o.source = Some(normalised.clone());
        }
        offenses.extend(found);
        if let Some(c) = corrector {
            let (again, text, _, err, _) = inspection_loop(path, display, Arc::from(c.rewrite()), cfg, selected, mode);
            offenses.extend(again);
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
) -> (Vec<Offense>, Arc<str>, bool, Option<String>, Vec<runner::Span>) {
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
        for o in &mut offenses {
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
/// write, and the final component is never followed as a symlink after that check.
// ponytail: extended attributes and ACLs are not copied to the renamed file.
// ponytail: a parent directory swapped for a symlink between the check and the write is not
// caught (needs openat with O_NOFOLLOW per component).
pub fn write_source(path: &Path, new: &str, roots: &[PathBuf]) -> std::io::Result<()> {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let target = fs::canonicalize(path)?;
    if outside_roots(&target, roots) {
        let msg = format!("{} resolves to {}, outside the project", path.display(), target.display());
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg));
    }
    let nofollow = || {
        let mut o = fs::OpenOptions::new();
        o.write(true).custom_flags(libc::O_NOFOLLOW);
        o
    };
    // File.open(path, 'w') fails on a file that is not writable; the rename must not get round that.
    drop(nofollow().open(&target)?);
    let meta = fs::metadata(&target)?;
    // SAFETY: geteuid has no preconditions.
    if meta.nlink() == 1 && meta.uid() == unsafe { libc::geteuid() } {
        // An unguessable name, created exclusively (O_EXCL: never an existing file or symlink) and
        // readable by the owner only until it has the file's own group and mode, before any content.
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let name = target.file_name().unwrap_or_default().to_string_lossy();
        let tmp = target.with_file_name(format!(".{name}.rbcop-{}-{n}-{nanos:x}.tmp", std::process::id()));
        if let Ok(mut f) = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp) {
            // The file's group, too (a group rbcop's user is not in cannot be given: write in place).
            if std::os::unix::fs::fchown(&f, None, Some(meta.gid())).is_ok() {
                let written = (|| {
                    f.set_permissions(meta.permissions())?;
                    f.write_all(new.as_bytes())?;
                    f.sync_all()?;
                    fs::rename(&tmp, &target)
                })();
                if written.is_err() {
                    let _ = fs::remove_file(&tmp);
                }
                return written;
            }
            drop(f);
            let _ = fs::remove_file(&tmp);
        }
    }
    nofollow().truncate(true).open(&target)?.write_all(new.as_bytes())
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
}
