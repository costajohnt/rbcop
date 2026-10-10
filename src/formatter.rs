//! Output formats. Each is byte-identical to the RuboCop 1.91.0 formatter of the same name.
//!
//! Formatters receive RuboCop's events (started / file_started / file_finished / finished) and
//! write into a String per event; `Set` forwards that text to each formatter's sink, so several
//! formatters sharing stdout interleave exactly as RuboCop's do.

mod report;
mod text;
pub mod yaml;

pub use report::ruby_platform;

use crate::config::{Resolved, relative_path};
use crate::cop::{Offense, Severity};
use std::fs::File;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Offense#status. rbcop does not autocorrect yet, so only Uncorrected/Unsupported occur today;
/// the others are rendered like RuboCop renders them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Unsupported,
    Uncorrected,
    Corrected,
    CorrectedWithTodo,
    Disabled,
    Todo,
}

impl Status {
    pub fn correctable(self) -> bool {
        !matches!(self, Status::Unsupported | Status::Disabled)
    }
    pub fn corrected(self) -> bool {
        matches!(self, Status::Corrected | Status::CorrectedWithTodo)
    }
    pub fn disabled(self) -> bool {
        matches!(self, Status::Disabled | Status::Todo)
    }
}

/// An offense as formatters see it (RuboCop::Cop::Offense): positions in characters, message
/// already annotated (cop name, details, style guide links).
#[derive(Clone, Debug)]
pub struct Off {
    pub severity: Severity,
    pub cop: &'static str,
    pub message: String,
    pub status: Status,
    /// 1-based.
    pub line: usize,
    /// 0-based, in characters.
    pub column: usize,
    pub last_line: usize,
    /// 0-based column of the range's end.
    pub last_column: usize,
    /// Range length in characters.
    pub length: usize,
    /// The first line of the range, without its newline.
    pub source_line: String,
}

impl Off {
    pub fn real_column(&self) -> usize {
        self.column + 1
    }
    pub fn real_last_column(&self) -> usize {
        if self.last_column == 0 { 1 } else { self.last_column }
    }
    pub fn single_line(&self) -> bool {
        self.line == self.last_line
    }
    /// Offense#highlighted_area as a char range of `source_line`.
    pub fn highlight(&self) -> (usize, usize) {
        let n = self.source_line.chars().count();
        let len = if self.single_line() {
            self.last_column.saturating_sub(self.column)
        } else {
            n.saturating_sub(self.column)
        };
        let begin = self.column.min(n);
        (begin, (self.column + len).min(n).max(begin))
    }
    /// Text of the highlighted area.
    pub fn highlighted(&self) -> String {
        let (b, e) = self.highlight();
        self.source_line.chars().skip(b).take(e - b).collect()
    }
}

/// 1-based (line, character column) of a byte offset, as RuboCop prints positions.
pub fn line_col(text: &str, off: usize) -> (usize, usize) {
    let off = off.min(text.len());
    let line_start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    (text[..off].matches('\n').count() + 1, text[line_start..off].chars().count() + 1)
}

/// Turn runner offenses into formatter offenses; `message` annotates each one.
pub fn offenses(text: &str, list: &[Offense], message: impl Fn(&Offense) -> String) -> Vec<Off> {
    let line_starts = |t: &str| {
        let mut starts = vec![0];
        starts.extend(t.match_indices('\n').map(|(i, _)| i + 1));
        starts
    };
    let starts = line_starts(text);
    let mut out: Vec<Off> = list
        .iter()
        .map(|o| {
            // Under autocorrect an offense may refer to the source of an earlier iteration.
            let own;
            let (text, starts): (&str, &[usize]) = match &o.source {
                Some(s) => {
                    own = line_starts(s);
                    (s, &own)
                }
                None => (text, &starts),
            };
            // Binary-encoded sources (magic comment) count columns in bytes, as Ruby does.
            let binary = crate::source::binary_encoding(text);
            let count = |s: &str| if binary { s.len() } else { s.chars().count() };
            let pos = |off: usize| {
                // Parser keeps counting columns past the end of the source (Layout/EndOfLine ranges
                // reach beyond the CRLF-normalised text), so an overflow carries into the column.
                let overflow = off.saturating_sub(text.len());
                let mut off = off.min(text.len());
                while !text.is_char_boundary(off) {
                    off -= 1;
                }
                let line = starts.partition_point(|&s| s <= off);
                let ls = starts[line - 1];
                (line, count(&text[ls..off]) + overflow, ls, off)
            };
            let (line, column, ls, start) = pos(o.start);
            let (last_line, last_column, _, end) = pos(o.end.max(o.start));
            let le = text[ls..].find('\n').map_or(text.len(), |i| ls + i);
            let status = if o.corrected {
                Status::Corrected
            } else if o.correctable {
                Status::Uncorrected
            } else {
                Status::Unsupported
            };
            if o.global {
                // Offense::NO_LOCATION: line 1, column 0, empty source line.
                let (line, column, length, source_line) = (1, 0, 0, String::new());
                let (last_line, last_column) = (1, 0);
                let (severity, cop, message) = (o.severity, o.cop, message(o));
                return Off {
                    severity,
                    cop,
                    message,
                    status,
                    line,
                    column,
                    last_line,
                    last_column,
                    length,
                    source_line,
                };
            }
            Off {
                severity: o.severity,
                cop: o.cop,
                message: message(o),
                status,
                line,
                column,
                last_line,
                last_column,
                length: count(&text[start..end]),
                source_line: text[ls..le].to_string(),
            }
        })
        .collect();
    // Offense#<=>: line, column, cop name, message, severity.
    out.sort_by(|a, b| {
        (a.line, a.column, a.cop, &a.message, a.severity).cmp(&(b.line, b.column, b.cop, &b.message, b.severity))
    });
    out
}

/// What MessageAnnotator needs from the command line.
#[derive(Clone, Copy, Default)]
pub struct Annotate {
    pub display_cop_names: Option<bool>,
    /// `--format json` was the last -f (RuboCop compares the raw option string).
    pub json: bool,
    pub extra_details: bool,
    pub display_style_guide: bool,
}

/// MessageAnnotator#annotate.
pub fn annotate(cfg: &Resolved, cop: &str, message: &str, a: Annotate) -> String {
    let all = cfg.all_cops();
    let cop_cfg = cfg.hash.map(cop).cloned().unwrap_or_default();
    let names = match a.display_cop_names {
        Some(v) => v,
        None => !a.json && all.truthy("DisplayCopNames"),
    };
    let mut m = if names { format!("{cop}: {message}") } else { message.to_string() };
    if let Some(d) = cop_cfg.str("Details").filter(|d| !d.is_empty() && (a.extra_details || all.truthy("ExtraDetails")))
    {
        m = format!("{m} {d}");
    }
    if a.display_style_guide || all.truthy("DisplayStyleGuide") {
        let mut urls = Vec::new();
        if let Some(url) = cop_cfg.str("StyleGuide").filter(|u| !u.is_empty()) {
            let dept = cop.rsplit_once('/').map_or("", |(d, _)| d);
            let base = cfg
                .hash
                .map(dept)
                .and_then(|d| d.str("StyleGuideBaseURL").map(String::from))
                .or_else(|| all.str("StyleGuideBaseURL").map(String::from));
            urls.push(match base {
                Some(b) if !b.is_empty() => uri_join(&b, url),
                _ => url.to_string(),
            });
        }
        for key in ["References", "Reference"] {
            match cop_cfg.get(key) {
                Some(crate::config::value::Value::Seq(items)) => {
                    urls.extend(items.iter().filter_map(|v| v.as_string()).filter(|s| !s.is_empty()))
                }
                Some(v) => urls.extend(v.as_string().filter(|s| !s.is_empty())),
                None => {}
            }
        }
        if !urls.is_empty() {
            m = format!("{m} ({})", urls.join(", "));
        }
    }
    m
}

/// URI.join for the shapes style guide links take.
fn uri_join(base: &str, url: &str) -> String {
    if url.contains("://") {
        return url.to_string();
    }
    let origin_end = base.find("://").and_then(|i| base[i + 3..].find('/').map(|j| i + 3 + j)).unwrap_or(base.len());
    if let Some(frag) = url.strip_prefix('#') {
        return format!("{}#{frag}", base.split('#').next().unwrap_or(base));
    }
    if url.starts_with('/') {
        return format!("{}{url}", &base[..origin_end]);
    }
    let dir = base[origin_end..]
        .rfind('/')
        .map_or(format!("{}/", &base[..origin_end]), |i| base[..origin_end + i + 1].to_string());
    format!("{dir}{url}")
}

/// `--fail-level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailLevel {
    Severity(Severity),
    Autocorrect,
}

/// Severity.minimum_to_fail.
pub fn minimum_to_fail(level: Option<FailLevel>, pwd: &Resolved) -> Severity {
    match level {
        Some(FailLevel::Severity(s)) => s,
        _ => pwd.all_cops().str("FailLevel").and_then(Severity::parse).unwrap_or(Severity::Refactor),
    }
}

/// Command-line state the formatters read.
#[derive(Clone, Default)]
pub struct Opts {
    pub color: Option<bool>,
    pub display_style_guide: bool,
    pub display_only_failed: bool,
    pub fail_level: Option<FailLevel>,
    /// `--only` (with Lint/Syntax added, as RuboCop does).
    pub only: Option<Vec<String>>,
    pub except: Vec<String>,
}

/// Shared, read-only context for one run.
pub struct Ctx {
    /// PathUtil.pwd
    pub pwd: String,
    pub opts: Opts,
    /// Rainbow.enabled after --color/--no-color.
    pub global_color: bool,
    pub pwd_config: Arc<Resolved>,
    /// The default configuration (ConfigLoader.default_configuration).
    pub default_config: Arc<Resolved>,
    /// Whether io/console is loaded in RuboCop (it is once ruby-progressbar is: fuubar/offenses).
    pub io_console: bool,
    pub stdout_tty: bool,
}

impl Ctx {
    /// PathUtil.smart_path
    pub fn smart_path(&self, path: &Path) -> String {
        let p = path.display().to_string();
        if p.starts_with(&self.pwd) { self.relative(path) } else { p }
    }

    /// PathUtil.relative_path(path, pwd)
    pub fn relative(&self, path: &Path) -> String {
        let p = path.display().to_string();
        if p.starts_with(&self.pwd) {
            p.get(self.pwd.len() + 1..).unwrap_or("").to_string()
        } else {
            relative_path(path, Path::new(&self.pwd))
        }
    }

    /// Cop.support_autocorrect? for the cops rbcop knows.
    pub fn supports_autocorrect(&self, cop: &str) -> bool {
        supports_autocorrect(cop)
    }

    /// Cop#safe_autocorrect? with the default configuration.
    pub fn safe_autocorrect(&self, cop: &str) -> bool {
        let c = self.default_config.hash.map(cop).cloned().unwrap_or_default();
        c.bool("Safe") != Some(false) && c.bool("SafeAutoCorrect") != Some(false)
    }
}

/// Cop.support_autocorrect?: the cop extends AutoCorrector in RuboCop (per docs/cop-inventory.tsv),
/// whether or not rbcop marks every offense of it correctable (LineLength only sometimes is).
pub fn supports_autocorrect(cop: &str) -> bool {
    include_str!("../docs/cop-inventory.tsv")
        .lines()
        .any(|l| l.split('\t').next() == Some(cop) && l.split('\t').nth(3).is_some_and(|m| m.contains("AutoCorrector")))
        || crate::cops::ALL.iter().any(|r| r.name == cop && r.cop.correctable())
}

/// One inspected file.
pub struct FileOut<'a> {
    pub path: &'a Path,
    pub offenses: Vec<Off>,
    pub config: &'a Resolved,
}

pub trait Formatter {
    /// Err carries the message of an exception RuboCop's formatter raises here.
    fn started(&mut self, _out: &mut String, _cx: &Ctx, _targets: &[PathBuf]) -> Result<(), String> {
        Ok(())
    }
    fn file_started(&mut self, _out: &mut String, _cx: &Ctx, _file: &Path) {}
    fn file_finished(&mut self, _out: &mut String, _cx: &Ctx, _file: &FileOut) {}
    fn finished(&mut self, _out: &mut String, _cx: &Ctx, _inspected: &[PathBuf]) {}
}

/// Rainbow: ANSI colours when enabled.
#[derive(Clone, Copy)]
pub struct Paint(pub bool);

impl Paint {
    pub fn wrap(self, s: &str, code: &str) -> String {
        if self.0 { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
    pub fn red(self, s: &str) -> String {
        self.wrap(s, "31")
    }
    pub fn green(self, s: &str) -> String {
        self.wrap(s, "32")
    }
    pub fn yellow(self, s: &str) -> String {
        self.wrap(s, "33")
    }
    pub fn magenta(self, s: &str) -> String {
        self.wrap(s, "35")
    }
    pub fn cyan(self, s: &str) -> String {
        self.wrap(s, "36")
    }
    /// SimpleTextFormatter::COLOR_FOR_SEVERITY
    pub fn severity(self, s: &str, sev: Severity) -> String {
        self.wrap(s, severity_color(sev))
    }
}

pub fn severity_color(sev: Severity) -> &'static str {
    match sev {
        Severity::Info => "38;5;188",
        Severity::Refactor | Severity::Convention => "33",
        Severity::Warning => "35",
        Severity::Error | Severity::Fatal => "31",
    }
}

/// TextUtil.pluralize
pub fn pluralize(n: usize, thing: &str, no_for_zero: bool) -> String {
    match n {
        0 if no_for_zero => format!("no {thing}s"),
        1 => format!("1 {thing}"),
        _ => format!("{n} {thing}s"),
    }
}

/// String#blank? (RuboCop core ext): empty or only whitespace/NUL.
pub fn blank(s: &str) -> bool {
    s.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\0'))
}

/// `msg.gsub(/`(.*?)`/)` (min = 0) or `/`(.+?)`/` (min = 1); `multiline` is the /m flag.
pub fn sub_backticks(msg: &str, multiline: bool, min: usize, f: impl Fn(&str) -> String) -> String {
    let c: Vec<char> = msg.chars().collect();
    let mut out = String::with_capacity(msg.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '`' {
            let first = (i + 1 + min).min(c.len());
            let mut found = None;
            if multiline || !c[i + 1..first].contains(&'\n') {
                for (j, &ch) in c.iter().enumerate().skip(first) {
                    if ch == '`' {
                        found = Some(j);
                        break;
                    }
                    if !multiline && ch == '\n' {
                        break;
                    }
                }
            }
            if let Some(j) = found {
                out.push_str(&f(&c[i + 1..j].iter().collect::<String>()));
                i = j + 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Unicode::DisplayWidth.of
pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| match c as u32 {
            0 | 5 | 7 | 8 | 10..=15 => 0,
            0..=0xa0 => 1,
            _ => unicode_width::UnicodeWidthChar::width(c).unwrap_or(1),
        })
        .sum()
}

/// Terminal columns of `fd`, if it is a terminal.
pub fn winsize(fd: i32) -> Option<usize> {
    // SAFETY: TIOCGWINSZ fills a winsize struct; we pass a valid pointer to one.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        (libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0).then_some(ws.ws_col as usize)
    }
}

/// FormatterSet::BUILTIN_FORMATTERS_FOR_KEYS
const KEYS: &[&str] = &[
    "[a]utogenconf",
    "[c]lang",
    "[e]macs",
    "[fi]les",
    "[fu]ubar",
    "[g]ithub",
    "[h]tml",
    "[j]son",
    "[ju]nit",
    "[m]arkdown",
    "[o]ffenses",
    "[pa]cman",
    "[p]rogress",
    "[q]uiet",
    "[sa]rif",
    "[s]imple",
    "[t]ap",
    "[w]orst",
];

/// FormatterSet#builtin_formatter_class: the formatter name for a `-f` key.
pub fn resolve(key: &str) -> Result<&'static str, String> {
    // Not a RuboCop formatter: emacs plus each offense's end position, matched by
    // scripts/oracle_formatter.rb, so the parity gate also checks ranges.
    if key == "rbcop-oracle" {
        return Ok("rbcop-oracle");
    }
    if key.starts_with(|c: char| c.is_ascii_uppercase()) || key.starts_with("::") {
        return Err(format!("custom formatter classes are not supported by rbcop: {key}"));
    }
    let m: Vec<&str> = KEYS
        .iter()
        .copied()
        .filter(|k| k.starts_with(&format!("[{key}]")) || key == k.replace(['[', ']'], ""))
        .collect();
    match m.as_slice() {
        [] => Err(format!("Formatter \"{key}\" not found.")),
        [one] => Ok(one),
        _ => Err(format!("Cannot determine formatter for \"{key}\"")),
    }
}

fn build(name: &str, color: bool, cx: &Ctx, tty: bool) -> Box<dyn Formatter> {
    use text::{Kind, Text};
    let p = Paint(color);
    match name.replace(['[', ']'], "").as_str() {
        "clang" => Box::new(Text::new(Kind::Clang, p, tty)),
        "emacs" => Box::new(report::Emacs),
        "rbcop-oracle" => Box::new(report::Oracle),
        "files" => Box::new(report::Files),
        "fuubar" => Box::new(Text::new(Kind::Fuubar, p, tty)),
        "github" => Box::new(report::Github::default()),
        "html" => Box::new(report::Html::default()),
        "json" => Box::new(report::Json::default()),
        "junit" => Box::new(report::Junit::default()),
        "markdown" => Box::new(report::Markdown::default()),
        "offenses" => Box::new(report::Offenses::new(tty, Paint(cx.global_color))),
        "pacman" => Box::new(Text::new(Kind::Pacman, p, tty)),
        "quiet" => Box::new(Text::new(Kind::Quiet, p, tty)),
        "sarif" => Box::new(report::Sarif::default()),
        "simple" => Box::new(Text::new(Kind::Simple, p, tty)),
        "tap" => Box::new(Text::new(Kind::Tap, p, tty)),
        "worst" => Box::new(report::Worst::default()),
        _ => Box::new(Text::new(Kind::Progress, p, tty)),
    }
}

enum Sink {
    Stdout,
    Stderr,
    File(File),
}

/// FormatterSet: every `-f`/`-o` pair, in command-line order.
pub struct Set {
    items: Vec<(Box<dyn Formatter>, Sink)>,
}

impl Set {
    /// `pairs` are (resolved formatter name, output file); `stderr` is --stderr.
    pub fn new(pairs: &[(&'static str, Option<PathBuf>)], cx: &Ctx, stderr: bool) -> Result<Set, String> {
        let mut items = Vec::new();
        for (name, path) in pairs {
            let (sink, tty) = match path {
                Some(p) => {
                    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                    }
                    let f = File::create(p).map_err(|e| format!("{}: {e}", p.display()))?;
                    let tty = f.is_terminal();
                    (Sink::File(f), tty)
                }
                None if stderr => (Sink::Stderr, std::io::stderr().is_terminal()),
                None => (Sink::Stdout, cx.stdout_tty),
            };
            // Colorizable#rainbow
            let color = match cx.opts.color {
                Some(c) => c,
                None => tty && cx.global_color,
            };
            items.push((build(name, color, cx, tty), sink));
        }
        Ok(Set { items })
    }

    fn each(&mut self, mut f: impl FnMut(&mut dyn Formatter, &mut String)) {
        let mut buf = String::new();
        for (fmt, sink) in &mut self.items {
            f(fmt.as_mut(), &mut buf);
            if !buf.is_empty() {
                // ponytail: write errors (closed pipe, full disk) are ignored, as `puts` to a closed pipe would end RuboCop anyway.
                let _ = match sink {
                    Sink::Stdout => std::io::stdout().write_all(buf.as_bytes()),
                    Sink::Stderr => std::io::stderr().write_all(buf.as_bytes()),
                    Sink::File(file) => file.write_all(buf.as_bytes()),
                };
                buf.clear();
            }
        }
    }

    /// Err: a formatter raised (RuboCop then still runs `finished` on every formatter).
    pub fn started(&mut self, cx: &Ctx, targets: &[PathBuf]) -> Result<(), String> {
        let mut err = None;
        self.each(|f, out| {
            if err.is_none() {
                err = f.started(out, cx, targets).err();
            }
        });
        err.map_or(Ok(()), Err)
    }
    pub fn file_started(&mut self, cx: &Ctx, file: &Path) {
        self.each(|f, out| f.file_started(out, cx, file));
    }
    pub fn file_finished(&mut self, cx: &Ctx, file: &FileOut) {
        self.each(|f, out| f.file_finished(out, cx, file));
    }
    pub fn finished(&mut self, cx: &Ctx, inspected: &[PathBuf]) {
        self.each(|f, out| f.finished(out, cx, inspected));
        let _ = std::io::stdout().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backticks() {
        let y = |s: &str| format!("<{s}>");
        assert_eq!(sub_backticks("a `b` c `d`", false, 0, y), "a <b> c <d>");
        assert_eq!(sub_backticks("``x`", false, 1, y), "<`x>");
        assert_eq!(sub_backticks("`a\nb` `c`", false, 0, y), "`a\nb< >c`");
        assert_eq!(sub_backticks("`a\nb` `c`", true, 0, y), "<a\nb> <c>");
    }

    #[test]
    fn global_offense_has_no_location() {
        let o = Offense {
            start: 0,
            end: 0,
            cop: "Bundler/GemFilename",
            message: "m".into(),
            severity: Severity::Convention,
            correctable: false,
            global: true,
            corrected: false,
            source: None,
        };
        let v = &offenses("source 'x'\n", &[o], |o| o.message.clone())[0];
        assert_eq!((v.line, v.real_column(), v.last_line, v.real_last_column(), v.length), (1, 1, 1, 1, 0));
        assert_eq!(v.source_line, "");
        assert_eq!(v.status, Status::Unsupported);
    }

    #[test]
    fn keys() {
        assert_eq!(resolve("p"), Ok("[p]rogress"));
        assert_eq!(resolve("pa"), Ok("[pa]cman"));
        assert_eq!(resolve("fi"), Ok("[fi]les"));
        assert_eq!(resolve("junit"), Ok("[ju]nit"));
        assert!(resolve("f").is_err());
    }

    #[test]
    fn style_guide_urls() {
        assert_eq!(uri_join("https://rubystyle.guide", "#x"), "https://rubystyle.guide#x");
        assert_eq!(uri_join("https://a.b/c/d", "e"), "https://a.b/c/e");
        assert_eq!(uri_join("https://a.b/c/d", "https://z"), "https://z");
    }
}
