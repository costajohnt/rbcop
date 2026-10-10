//! Autocorrect (`-a` / `-A`): RuboCop's Team corrector collation (lib/rubocop/cop/team.rb) and the
//! Runner's inspect-correct-reinspect loop (lib/rubocop/runner.rb, `do_inspection_loop`).

pub mod corrector;
pub mod tree_rewriter;

use crate::config::value::Value;
use crate::config::{CopConfig, Resolved};
use crate::cop::Offense;
use crate::formatter::line_col;
use crate::runner::{self, FileReport};
use corrector::Corrector;
use std::collections::{HashMap, HashSet};
use std::path::Path;
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

/// Cop::AutocorrectLogic#autocorrect? for a cop that supports autocorrection (`Cop::correctable`).
pub fn cop_autocorrects(cfg: &CopConfig, mode: Mode) -> bool {
    // autocorrect_enabled?
    if matches!(cfg.get("AutoCorrect"), Some(Value::Bool(false))) || cfg.str("AutoCorrect") == Some("disabled") {
        return false;
    }
    mode == Mode::All || (cfg.bool("Safe").unwrap_or(true) && cfg.bool("SafeAutoCorrect").unwrap_or(true))
}

/// Team#collate_corrections: merge each cop's corrector in registry order, skipping a cop whose
/// corrections clobber what is already merged or that an earlier merged cop is incompatible with.
pub fn collate(mut by_cop: Vec<(&'static str, Corrector)>, source: Arc<str>) -> Option<Corrector> {
    by_cop.sort_by_key(|(name, _)| REGISTRY.order.get(name).copied().unwrap_or(usize::MAX));
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
    // parser's Source::Buffer normalises CRLF; corrections are applied to (and written from) that.
    let text = if text.contains("\r\n") { text.replace("\r\n", "\n") } else { text };
    let mut current: Arc<str> = Arc::from(text);
    let mut seen: Vec<Arc<str>> = Vec::new();
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
        let (mut offenses, corrector) = runner::analyze_for_correction(path, display, &current, cfg, selected, mode);
        for o in &mut offenses {
            o.source = Some(current.clone());
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
    let report = FileReport { offenses: uniq_sorted(by_iteration.concat()), text: current.to_string() };
    Corrected { source: changed.then(|| current.to_string()), report, error }
}

/// Autocorrect one file on disk: inspect, correct, write the result back, report.
pub fn correct_file(
    path: &Path,
    display: &str,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
) -> FileReport {
    let bytes = std::fs::read(path).unwrap_or_default();
    let Ok(text) = String::from_utf8(bytes) else {
        // ponytail: never rewrite a file that is not valid UTF-8; it is only inspected.
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned();
        return runner::inspect(path, display, text, cfg, selected);
    };
    let c = correct(path, display, text, cfg, selected, mode);
    if let Some(new) = &c.source
        && let Err(e) = std::fs::write(path, new)
    {
        eprintln!("rbcop: cannot write {}: {e}", path.display());
    }
    if let Some(e) = &c.error {
        eprintln!("{e}");
    }
    c.report
}

fn infinite_loop(path: &Path, iterations: &[Vec<Offense>]) -> String {
    let cause: Vec<String> = iterations
        .iter()
        .map(|offenses| {
            let mut names: Vec<&str> = Vec::new();
            for o in offenses {
                if !names.contains(&o.cop) {
                    names.push(o.cop);
                }
            }
            names.join(", ")
        })
        .collect();
    format!("Infinite loop detected in {} and caused by {}", path.display(), cause.join(" -> "))
}

/// `offenses.flatten.uniq` then `sort`: Offense equality and order are by
/// (line, column, cop name, message, severity).
fn uniq_sorted(offenses: Vec<Offense>) -> Vec<Offense> {
    let key = |o: &Offense| {
        let (line, col) = line_col(o.source.as_deref().unwrap_or(""), o.start);
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
