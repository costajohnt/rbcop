//! Output formats. Each must be byte-identical to the RuboCop formatter of the same name.

use crate::cop::Offense;
use crate::runner::FileReport;
use std::fmt::Write;
use std::path::Path;

pub struct Inspected<'a> {
    /// Absolute path.
    pub path: &'a Path,
    /// Path as RuboCop prints it in relative contexts (relative to cwd when below it).
    pub relative: String,
    pub report: &'a FileReport,
}

pub fn line_col(text: &str, off: usize) -> (usize, usize) {
    let off = off.min(text.len());
    let line_start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    (text[..off].matches('\n').count() + 1, text[line_start..off].chars().count() + 1)
}

fn tag(o: &Offense) -> &'static str {
    if o.correctable { "[Correctable] " } else { "" }
}

/// `-f emacs`: `/abs/path:line:col: C: [Correctable] Cop/Name: message`
pub fn emacs(files: &[Inspected]) -> String {
    let mut out = String::new();
    for f in files {
        for o in &f.report.offenses {
            let (line, col) = line_col(&f.report.text, o.start);
            let msg = o.message.replace('\n', " ");
            let _ =
                writeln!(out, "{}:{line}:{col}: {}: {}{}: {msg}", f.path.display(), o.severity.code(), tag(o), o.cop);
        }
    }
    out
}

pub const FORMATS: &[&str] = &["emacs"];
