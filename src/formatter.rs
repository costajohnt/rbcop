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
use std::collections::HashMap;
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
    pub source_line: Arc<str>,
    /// `add_global_offense`: the location is Offense::NO_LOCATION, a PseudoSourceRange.
    pub global: bool,
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

/// Offsets where each line of `text` starts, for `line_col_at`.
pub fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect()
}

/// `line_col` with the line starts of `text` indexed once (`line_starts`): no rescan of the text
/// before `off` per lookup.
pub fn line_col_at(text: &str, starts: &[usize], off: usize) -> (usize, usize) {
    let off = off.min(text.len());
    let line = starts.partition_point(|&s| s <= off);
    (line, text[starts[line - 1]..off].chars().count() + 1)
}

/// Position lookups in one source for `offenses`. Everything here is computed once per source:
/// rescanning the text (or a long line) per offense is quadratic when one long line carries many
/// offenses (`format('%s%s...', ...)` with 100k sequences took 25 s and 4.8 GB).
struct Positions<'a> {
    text: &'a str,
    starts: Vec<usize>,
    /// Binary-encoded sources (magic comment) count columns in bytes, as Ruby does.
    binary: bool,
    /// Characters before every 256th byte.
    chars: Vec<usize>,
    lines: HashMap<usize, Arc<str>>,
}

impl<'a> Positions<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        let mut chars = vec![0];
        for chunk in text.as_bytes().chunks(256) {
            chars.push(chars.last().unwrap() + char_starts(chunk));
        }
        let binary = crate::source::binary_encoding(text);
        Positions { text, starts, binary, chars, lines: HashMap::new() }
    }

    /// Columns between two char boundaries.
    fn count(&self, from: usize, to: usize) -> usize {
        let before = |off: usize| self.chars[off / 256] + char_starts(&self.text.as_bytes()[off / 256 * 256..off]);
        if self.binary { to - from } else { before(to) - before(from) }
    }

    /// (line, column, line start, offset clamped to a char boundary).
    fn pos(&self, off: usize) -> (usize, usize, usize, usize) {
        // Parser keeps counting columns past the end of the source (Layout/EndOfLine ranges
        // reach beyond the CRLF-normalised text), so an overflow carries into the column.
        let overflow = off.saturating_sub(self.text.len());
        let mut off = off.min(self.text.len());
        while !self.text.is_char_boundary(off) {
            off -= 1;
        }
        let line = self.starts.partition_point(|&s| s <= off);
        let ls = self.starts[line - 1];
        (line, self.count(ls, off) + overflow, ls, off)
    }

    /// The line starting at `ls`, without its newline; shared by every offense on it.
    fn line(&mut self, ls: usize) -> Arc<str> {
        let text = self.text;
        let line = self.lines.entry(ls).or_insert_with(|| {
            let le = text[ls..].find('\n').map_or(text.len(), |i| ls + i);
            Arc::from(&text[ls..le])
        });
        line.clone()
    }
}

/// UTF-8 characters starting in `bytes` (bytes that are not continuation bytes).
fn char_starts(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b & 0xC0 != 0x80).count()
}

/// Turn runner offenses into formatter offenses; `message` annotates each one.
pub fn offenses(text: &str, list: &[Offense], message: impl Fn(&Offense) -> String) -> Vec<Off> {
    let mut main = Positions::new(text);
    // Under autocorrect an offense may refer to the source of an earlier iteration.
    let mut earlier: HashMap<*const u8, Positions> = HashMap::new();
    let mut out: Vec<Off> = list
        .iter()
        .map(|o| {
            let p = match &o.source {
                Some(s) => earlier.entry(s.as_ptr()).or_insert_with(|| Positions::new(s)),
                None => &mut main,
            };
            let (line, column, ls, start) = p.pos(o.start);
            let (last_line, last_column, _, end) = p.pos(o.end.max(o.start));
            // Range#length is end_pos - begin_pos, past the end of the source included.
            let overflow = o.end.max(o.start).saturating_sub(p.text.len());
            let status = if o.corrected {
                Status::Corrected
            } else if o.correctable {
                Status::Uncorrected
            } else {
                Status::Unsupported
            };
            if o.global {
                // Offense::NO_LOCATION: line 1, column 0, empty source line.
                let (line, column, length, source_line) = (1, 0, 0, Arc::from(""));
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
                    global: true,
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
                length: p.count(start, end) + overflow,
                source_line: p.line(ls),
                global: false,
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
            // MessageAnnotator.style_guide_urls: resolved once per process for each StyleGuide
            // value, so the first offense with an anchor fixes its base for every cop (#508).
            // ponytail: filled in report order (files, then sorted offenses); RuboCop fills it as
            // offenses are added, disabled ones and correcting passes included.
            static RESOLVED: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
            let mut resolved = RESOLVED.lock().unwrap_or_else(|e| e.into_inner());
            let full = match resolved.iter().find(|(k, _)| k == url) {
                Some((_, full)) => full.clone(),
                None => {
                    let dept = cop.rsplit_once('/').map_or("", |(d, _)| d);
                    let base = cfg
                        .hash
                        .map(dept)
                        .and_then(|d| d.str("StyleGuideBaseURL").map(String::from))
                        .or_else(|| all.str("StyleGuideBaseURL").map(String::from));
                    let full = match base {
                        Some(b) if !b.is_empty() => uri_join(&b, url),
                        _ => url.to_string(),
                    };
                    resolved.push((url.to_string(), full.clone()));
                    full
                }
            };
            urls.push(full);
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
    /// `@options[:safe_autocorrect]` (-a): the summary points at `rubocop -A`.
    pub safe_autocorrect: bool,
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
        || crate::helpers::cop_registry::foreign_autocorrect(cop)
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
    /// Rainbow::StringUtils.wrap_with_sgr: the code goes after any SGR codes the string starts
    /// with, and no reset is added to a string that already ends with one.
    pub fn wrap(self, s: &str, code: &str) -> String {
        if !self.0 {
            return s.to_string();
        }
        let mut lead = 0;
        while let Some(rest) = s[lead..].strip_prefix("\x1b[") {
            let digits = rest.bytes().take_while(|b| b.is_ascii_digit() || *b == b';').count();
            if digits == 0 || rest.as_bytes().get(digits) != Some(&b'm') {
                break;
            }
            lead += 2 + digits + 1;
        }
        let reset = if s.ends_with("\x1b[0m") { "" } else { "\x1b[0m" };
        format!("{}\x1b[{code}m{}{reset}", &s[..lead], &s[lead..])
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
    File(File, PathBuf),
}

/// FormatterSet: every `-f`/`-o` pair, in command-line order.
pub struct Set {
    items: Vec<(Box<dyn Formatter>, Sink)>,
    /// The first failed write, worded as Ruby's.
    error: Option<String>,
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
                    (Sink::File(f, p.clone()), tty)
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
        Ok(Set { items, error: None })
    }

    fn each(&mut self, mut f: impl FnMut(&mut dyn Formatter, &mut String)) {
        let mut buf = String::new();
        for (fmt, sink) in &mut self.items {
            f(fmt.as_mut(), &mut buf);
            if !buf.is_empty() {
                let (written, name) = match sink {
                    Sink::Stdout => (std::io::stdout().write_all(buf.as_bytes()), "<STDOUT>".to_string()),
                    Sink::Stderr => (std::io::stderr().write_all(buf.as_bytes()), "<STDERR>".to_string()),
                    Sink::File(file, path) => (file.write_all(buf.as_bytes()), path.display().to_string()),
                };
                // A closed pipe is ignored (output is going nowhere anyway); any other failure (a full
                // disk, a file size limit) is an error that fails the run, as RuboCop's raise does.
                if let Err(e) = written
                    && e.kind() != std::io::ErrorKind::BrokenPipe
                    && self.error.is_none()
                {
                    let msg = e.to_string();
                    let msg = msg.split(" (os error").next().unwrap_or(&msg).to_string();
                    self.error = Some(format!("{msg} @ rb_sys_fail_on_write - {name}"));
                }
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
    /// The first output write that failed since the last call.
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
    pub fn finished(&mut self, cx: &Ctx, inspected: &[PathBuf]) {
        self.each(|f, out| f.finished(out, cx, inspected));
        let _ = std::io::stdout().flush();
    }
}

/// Cop::Documentation.url_for with the department's DocumentationBaseURL / DocumentationExtension;
/// None for a plugin cop whose department sets no base URL (only RuboCop's own cops fall back to
/// docs.rubocop.org/rubocop).
pub fn docs_url(config: &crate::config::Resolved, cop: &str) -> Option<String> {
    let dept = cop.rsplit_once('/').map_or("", |(d, _)| d);
    let dept_cfg = config.hash.map(dept).cloned().unwrap_or_default();
    let base = match dept_cfg.str("DocumentationBaseURL") {
        Some(b) => b,
        None if crate::helpers::cop_registry::is_builtin(cop) => "https://docs.rubocop.org/rubocop",
        None => return None,
    };
    let ext = dept_cfg.str("DocumentationExtension").unwrap_or(".html");
    let fragment: String = cop.to_lowercase().chars().filter(char::is_ascii_lowercase).collect();
    Some(format!("{base}/cops_{}{ext}#{fragment}", dept.to_lowercase().replace('/', "_")))
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
        assert_eq!(&*v.source_line, "");
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
