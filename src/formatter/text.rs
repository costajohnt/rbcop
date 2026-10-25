//! SimpleTextFormatter and its family: simple, quiet, clang, progress, tap, fuubar, pacman.

use super::{Ctx, FileOut, Formatter, Off, Paint, blank, display_width, pluralize, sub_backticks, winsize};
use crate::cop::Severity;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub(super) enum Kind {
    Simple,
    Quiet,
    Clang,
    Progress,
    /// AutoGenConfigFormatter: progress without the offenses.
    AutoGenConf,
    Tap,
    Fuubar,
    Pacman,
}

pub(super) struct Text {
    kind: Kind,
    p: Paint,
    tty: bool,
    offenses: usize,
    corrections: usize,
    correctable: usize,
    /// progress: files with offenses, reported at the end.
    stored: Vec<(PathBuf, Vec<Off>)>,
    /// tap: running test number.
    count: usize,
    /// fuubar
    bar: Option<Bar>,
    severest: Option<Severity>,
    /// pacman
    line: String,
    total: usize,
    reps: usize,
    cols: usize,
    global: Paint,
}

const GHOST: &str = "ᗣ";

impl Text {
    pub(super) fn new(kind: Kind, p: Paint, tty: bool) -> Text {
        Text {
            kind,
            p,
            tty,
            offenses: 0,
            corrections: 0,
            correctable: 0,
            stored: Vec::new(),
            count: 1,
            bar: None,
            severest: None,
            line: String::new(),
            total: 0,
            reps: 0,
            cols: 80,
            global: Paint(false),
        }
    }

    fn count_stats(&mut self, offenses: &[Off]) {
        self.offenses += offenses.len();
        let corrected = offenses.iter().filter(|o| o.status.corrected()).count();
        self.corrections += corrected;
        self.correctable += offenses.iter().filter(|o| o.status.correctable()).count() - corrected;
    }

    /// SimpleTextFormatter::Report#summary
    fn summary(&self, out: &mut String, cx: &Ctx, files: usize) {
        let p = self.p;
        let files = pluralize(files, "file", false);
        let t = pluralize(self.offenses, "offense", true);
        let offenses = if self.offenses == 0 { p.green(&t) } else { p.red(&t) };
        let t = pluralize(self.corrections, "offense", false);
        let corrections = if self.corrections == self.offenses { p.green(&t) } else { p.cyan(&t) };
        let correctable = if cx.opts.safe_autocorrect {
            format!(
                "{} can be corrected with `rubocop -A`",
                p.yellow(&pluralize(self.correctable, "more offense", false))
            )
        } else {
            format!("{} autocorrectable", p.yellow(&pluralize(self.correctable, "offense", false)))
        };
        let s = match (self.corrections > 0, self.correctable > 0) {
            (true, true) => {
                format!("{files} inspected, {offenses} detected, {corrections} corrected, {correctable}")
            }
            (true, false) => format!("{files} inspected, {offenses} detected, {corrections} corrected"),
            (false, true) => format!("{files} inspected, {offenses} detected, {correctable}"),
            (false, false) => format!("{files} inspected, {offenses} detected"),
        };
        let _ = write!(out, "\n{s}\n");
    }

    fn severity_code(&self, o: &Off) -> String {
        self.p.severity(&o.severity.code().to_string(), o.severity)
    }

    /// SimpleTextFormatter#message
    fn message(&self, o: &Off) -> String {
        let p = self.p;
        let tag = if o.status.disabled() {
            p.magenta("[Suppressed] ")
        } else if o.status == super::Status::CorrectedWithTodo {
            p.green("[Todo] ")
        } else if o.status.corrected() {
            p.green("[Corrected] ")
        } else if o.status.correctable() {
            p.yellow("[Correctable] ")
        } else {
            String::new()
        };
        format!("{tag}{}", sub_backticks(&o.message, true, 0, |s| p.yellow(s)))
    }

    /// TapFormatter#message
    fn tap_message(o: &Off) -> String {
        let tag = if o.status.disabled() {
            "[Suppressed] "
        } else if o.status == super::Status::CorrectedWithTodo {
            "[Todo] "
        } else if o.status.corrected() {
            "[Corrected] "
        } else if o.status.correctable() {
            "[Correctable] "
        } else {
            ""
        };
        format!("{tag}{}", sub_backticks(&o.message, false, 0, |s| s.to_string()))
    }

    fn report_file(&self, out: &mut String, cx: &Ctx, file: &Path, offenses: &[Off]) {
        match self.kind {
            Kind::Simple | Kind::Quiet => {
                let _ = writeln!(out, "{}", self.p.yellow(&format!("== {} ==", cx.smart_path(file))));
                for o in offenses {
                    let _ = writeln!(
                        out,
                        "{}:{:>3}:{:>3}: {}",
                        self.severity_code(o),
                        o.line,
                        o.real_column(),
                        self.message(o)
                    );
                }
            }
            _ => offenses.iter().for_each(|o| self.report_offense(out, cx, file, o)),
        }
    }

    /// ClangStyleFormatter#report_offense (and TapFormatter's, with `# ` in front).
    fn report_offense(&self, out: &mut String, cx: &Ctx, file: &Path, o: &Off) {
        let tap = matches!(self.kind, Kind::Tap);
        let prefix = if tap { "# " } else { "" };
        let msg = if tap { Self::tap_message(o) } else { self.message(o) };
        let path = self.p.cyan(&cx.smart_path(file));
        let _ = writeln!(out, "{prefix}{path}:{}:{}: {}: {msg}", o.line, o.real_column(), self.severity_code(o));
        if blank(&o.source_line) {
            return;
        }
        if o.single_line() {
            let _ = writeln!(out, "{prefix}{}", o.source_line);
        } else {
            let _ = writeln!(out, "{prefix}{} {}", o.source_line, self.p.yellow("..."));
        }
        let space: String = o.source_line.chars().take(o.highlight().0).collect();
        let tabs: String = space.chars().filter(|&c| c == '\t').collect();
        let rest: String = space.chars().filter(|&c| c != '\t').collect();
        let carets = "^".repeat(display_width(&o.highlighted()));
        let _ = writeln!(out, "{prefix}{tabs}{}{carets}", " ".repeat(display_width(&rest)));
    }

    /// FuubarStyleFormatter#with_color
    fn with_color(&mut self, out: &mut String, f: impl FnOnce(&mut Bar, &mut String)) {
        let Some(bar) = self.bar.as_mut() else { return };
        if self.p.0 {
            let code = self.severest.map_or("32", super::severity_color);
            let _ = write!(out, "\x1b[{code}m");
            f(bar, out);
            out.push_str("\x1b[0m");
        } else {
            f(bar, out);
        }
    }

    fn pacman(&self) -> String {
        self.global.wrap("ᗧ", "33m\x1b[1")
    }
    fn pacdot(&self) -> String {
        self.global.wrap("•", "33m\x1b[1")
    }

    /// PacmanFormatter#update_progress_line
    fn update_progress_line(&mut self) {
        let n = if self.total <= self.cols {
            self.total
        } else if self.total / self.cols != self.reps {
            self.cols
        } else {
            self.total - self.cols * self.reps
        };
        self.line = self.pacdot().repeat(n);
    }

    /// PacmanFormatter#step
    fn step(&mut self, out: &mut String, ch: &str) {
        let (pac, dot) = (self.pacman(), self.pacdot());
        let hit = match (self.line.find(&pac), self.line.find(&dot)) {
            (Some(a), Some(b)) if b < a => Some((b, dot.len())),
            (Some(a), _) => Some((a, pac.len())),
            (None, Some(b)) => Some((b, dot.len())),
            (None, None) => None,
        };
        if let Some((i, len)) = hit {
            self.line.replace_range(i..i + len, ch);
        }
        let _ = write!(out, "{}\r", self.line);
        if self.line.ends_with(GHOST) || self.line.ends_with('.') {
            self.reps += 1;
            out.push('\n');
            self.update_progress_line();
        }
    }
}

impl Formatter for Text {
    fn started(&mut self, out: &mut String, cx: &Ctx, targets: &[PathBuf]) -> Result<(), String> {
        self.offenses = 0;
        self.corrections = 0;
        self.correctable = 0;
        match self.kind {
            Kind::Progress | Kind::AutoGenConf => {
                self.stored.clear();
                let _ = writeln!(out, "Inspecting {}", pluralize(targets.len(), "file", false));
            }
            Kind::Tap => {
                self.count = 1;
                let _ = writeln!(out, "1..{}", targets.len());
            }
            Kind::Fuubar => {
                self.severest = None;
                self.bar = Some(Bar::new(self.tty, targets.len()));
                self.with_color(out, |bar, out| bar.refresh(out));
            }
            Kind::Pacman => {
                self.global = Paint(cx.global_color);
                self.total = targets.len();
                let _ = writeln!(out, "Eating {}", pluralize(targets.len(), "file", false));
                // PacmanFormatter#cols calls $stdout.winsize, which only exists once something has
                // loaded io/console, and raises unless stdout is a terminal.
                if !cx.io_console {
                    return Err("undefined method 'winsize' for an instance of IO".into());
                }
                if !cx.stdout_tty {
                    return Err("Inappropriate ioctl for device - <STDOUT>".into());
                }
                self.cols = winsize(1).filter(|&w| w > 0).unwrap_or(80);
                self.update_progress_line();
            }
            _ => {}
        }
        Ok(())
    }

    fn file_started(&mut self, out: &mut String, _cx: &Ctx, _file: &Path) {
        if matches!(self.kind, Kind::Pacman) {
            let pac = self.pacman();
            self.step(out, &pac);
        }
    }

    fn file_finished(&mut self, out: &mut String, cx: &Ctx, f: &FileOut) {
        let offs = &f.offenses;
        match self.kind {
            Kind::Simple | Kind::Quiet | Kind::Clang => {
                if !offs.is_empty() {
                    self.count_stats(offs);
                    self.report_file(out, cx, f.path, offs);
                }
            }
            Kind::Progress | Kind::AutoGenConf => {
                let mark = match offs.iter().rev().max_by_key(|o| o.severity) {
                    None => self.p.green("."),
                    Some(o) => self.severity_code(o),
                };
                if !offs.is_empty() {
                    self.count_stats(offs);
                    self.stored.push((f.path.to_path_buf(), offs.clone()));
                }
                out.push_str(&mark);
            }
            Kind::Tap => {
                let path = cx.smart_path(f.path);
                if offs.is_empty() {
                    let _ = writeln!(out, "ok {} - {path}", self.count);
                } else {
                    let _ = writeln!(out, "not ok {} - {path}", self.count);
                    self.count_stats(offs);
                    self.report_file(out, cx, f.path, offs);
                }
                self.count += 1;
            }
            Kind::Fuubar => {
                self.count_stats(offs);
                let worst = offs.iter().filter(|o| !o.status.corrected()).map(|o| o.severity).max();
                if let Some(w) = worst {
                    self.severest = Some(self.severest.map_or(w, |s| s.max(w)));
                }
                if !offs.is_empty() {
                    if let Some(bar) = self.bar.as_mut() {
                        bar.clear(out);
                    }
                    self.report_file(out, cx, f.path, offs);
                }
                self.with_color(out, |bar, out| bar.increment(out));
            }
            Kind::Pacman => {
                if !offs.is_empty() {
                    self.count_stats(offs);
                }
                match offs.last() {
                    None => self.step(out, "."),
                    Some(o) => {
                        let ghost = self.p.severity(GHOST, o.severity);
                        self.step(out, &ghost);
                    }
                }
                self.report_file(out, cx, f.path, offs);
            }
        }
    }

    fn finished(&mut self, out: &mut String, cx: &Ctx, inspected: &[PathBuf]) {
        match self.kind {
            Kind::Progress => {
                out.push('\n');
                if !self.stored.is_empty() {
                    out.push_str("\nOffenses:\n\n");
                    for (file, offs) in &self.stored {
                        offs.iter().for_each(|o| self.report_offense(out, cx, file, o));
                    }
                }
                self.summary(out, cx, inspected.len());
            }
            Kind::AutoGenConf => {
                out.push('\n');
                self.summary(out, cx, inspected.len());
            }
            Kind::Quiet if self.offenses == 0 => {}
            _ => self.summary(out, cx, inspected.len()),
        }
    }
}

/// The parts of ruby-progressbar FuubarStyleFormatter (and the offenses formatter on a terminal)
/// use. On a terminal the ETA is wall-clock based, so only the non-terminal output is exact.
// ponytail: refreshes are never throttled; ruby-progressbar skips refreshes <10ms apart, which only
// changes where a non-terminal bar line breaks when a file takes under 10ms in RuboCop.
pub(super) struct Bar {
    tty: bool,
    width: usize,
    total: usize,
    progress: usize,
    last: usize,
    sized: bool,
    start: Instant,
    phrase: &'static str,
}

impl Bar {
    pub(super) fn new(tty: bool, total: usize) -> Bar {
        Bar {
            tty,
            width: bar_width(tty),
            total,
            progress: 0,
            last: 0,
            sized: false,
            start: Instant::now(),
            phrase: if total == 1 { "file" } else { "files" },
        }
    }

    fn finished(&self) -> bool {
        self.progress >= self.total
    }

    fn render(&self) -> String {
        let pct = (self.progress * 100).checked_div(self.total).unwrap_or(100);
        if !self.tty {
            let done = self.width.saturating_sub(12) * pct / 100;
            return format!("Progress: |{}|", "=".repeat(done));
        }
        let secs = self.start.elapsed().as_secs();
        let eta = if self.finished() {
            format!("Time: {}", hms(secs))
        } else if self.progress == 0 {
            " ETA: ??:??:??".to_string()
        } else {
            format!(" ETA: {}", hms(secs * (self.total - self.progress) as u64 / self.progress as u64))
        };
        let (c, t, ph) = (self.progress, self.total, self.phrase);
        let len = self.width.saturating_sub(format!(" {c}/{t} {ph} |>| {eta} ").chars().count());
        let done = len * pct / 100;
        let w = if done < 5 { "=".repeat(done) } else { center(&format!(" {pct} "), done) };
        format!(" {c}/{t} {ph} |{w}>{}| {eta} ", " ".repeat(len - done))
    }

    pub(super) fn refresh(&mut self, out: &mut String) {
        if !self.sized {
            self.sized = true;
            self.clear(out);
        }
        let s = self.render();
        if self.tty {
            out.push_str(&s);
        } else {
            let s = if self.finished() { &s[..] } else { &s[..s.len() - 1] };
            out.push_str(s.get(self.last..).unwrap_or(""));
            self.last = s.len();
        }
        out.push_str(if self.finished() {
            "\n"
        } else if self.tty {
            "\r"
        } else {
            ""
        });
    }

    pub(super) fn clear(&mut self, out: &mut String) {
        if self.tty {
            out.push_str(&" ".repeat(self.width));
            out.push('\r');
        } else {
            self.last = 0;
            out.push('\n');
        }
    }

    pub(super) fn increment(&mut self, out: &mut String) {
        if !self.finished() {
            self.progress += 1;
        }
        self.refresh(out);
    }
}

fn hms(s: u64) -> String {
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// String#center with '='.
fn center(s: &str, width: usize) -> String {
    let pad = width.saturating_sub(s.chars().count());
    format!("{}{s}{}", "=".repeat(pad / 2), "=".repeat(pad - pad / 2))
}

/// ProgressBar::Calculators::Length#calculate_length
fn bar_width(tty: bool) -> usize {
    if let Ok(v) = std::env::var("RUBY_PROGRESS_BAR_LENGTH") {
        return v.trim().chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0);
    }
    let w = if tty {
        winsize(1)
    } else {
        use std::os::fd::AsRawFd;
        std::fs::File::open("/dev/tty").ok().and_then(|f| winsize(f.as_raw_fd()))
    };
    w.filter(|&w| w >= 20).unwrap_or(80)
}
