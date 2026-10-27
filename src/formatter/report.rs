//! The non-text formatters: emacs, files, github, html, json, junit, markdown, offenses, sarif, worst.

use super::text::Bar;
use super::{Ctx, FileOut, Formatter, Off, Paint, blank, minimum_to_fail, pluralize, sub_backticks};
use crate::config::{RUBOCOP_VERSION, relative_path};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// RUBY_ENGINE / RUBY_VERSION / RUBY_PATCHLEVEL of the Ruby that RuboCop 1.91.0 is checked against.
const RUBY_VERSION: &str = "4.0.6";
const RUBY_PATCHLEVEL: &str = "0";

/// RUBY_PLATFORM, e.g. `arm64-darwin25` or `x86_64-linux`.
// ponytail: derived from the running OS; a Ruby built on an older macOS reports its build version.
pub fn ruby_platform() -> String {
    let arch = match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "arm64",
        (a, _) => a,
    };
    if std::env::consts::OS != "macos" {
        return format!("{arch}-{}", std::env::consts::OS);
    }
    // SAFETY: uname fills the struct we pass; release is NUL-terminated.
    let release = unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        libc::uname(&mut u);
        std::ffi::CStr::from_ptr(u.release.as_ptr()).to_string_lossy().into_owned()
    };
    format!("{arch}-darwin{}", release.split('.').next().unwrap_or(""))
}

/// Ruby's JSON.generate string escaping.
pub fn json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Escape `& < >` for HTML text nodes. RuboCop leaves message text outside backtick spans
/// unescaped (an XSS vector when a message quotes source); rbcop deliberately escapes these three,
/// which only changes output when such text contains them. Quotes stay, so ordinary messages
/// ("Don't ...") are byte-identical.
fn html_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Undo html_text inside a code span before escaping it the RuboCop way.
fn unescape_text(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// CGI.escapeHTML
fn html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// `-f emacs`: `/abs/path:line:col: C: [Correctable] Cop/Name: message`
pub(super) struct Emacs;

impl Formatter for Emacs {
    fn file_finished(&mut self, out: &mut String, _cx: &Ctx, f: &FileOut) {
        for o in &f.offenses {
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
            let msg = format!("{tag}{}", o.message).replace('\n', " ");
            let _ = writeln!(out, "{}:{}:{}: {}: {msg}", f.path.display(), o.line, o.real_column(), o.severity.code());
        }
    }
}

/// `-f rbcop-oracle`: emacs format with the range end (`path:line:col:last_line:last_col: ...`),
/// as scripts/oracle_formatter.rb prints it from RuboCop.
pub(super) struct Oracle;

impl Formatter for Oracle {
    fn file_finished(&mut self, out: &mut String, _cx: &Ctx, f: &FileOut) {
        for o in &f.offenses {
            let tag = if o.status.correctable() { "[Correctable] " } else { "" };
            let msg = format!("{tag}{}", o.message).replace('\n', " ");
            let _ = writeln!(
                out,
                "{}:{}:{}:{}:{}: {}: {msg}",
                f.path.display(),
                o.line,
                o.real_column(),
                o.last_line,
                o.last_column,
                o.severity.code()
            );
        }
    }
}

/// `-f files`: absolute paths of files with offenses.
pub(super) struct Files;

impl Formatter for Files {
    fn file_finished(&mut self, out: &mut String, _cx: &Ctx, f: &FileOut) {
        if !f.offenses.is_empty() {
            let _ = writeln!(out, "{}", f.path.display());
        }
    }
}

/// `-f github`: GitHub Actions workflow commands.
#[derive(Default)]
pub(super) struct Github {
    files: Vec<(PathBuf, Vec<Off>)>,
}

impl Formatter for Github {
    fn file_finished(&mut self, _out: &mut String, _cx: &Ctx, f: &FileOut) {
        if !f.offenses.is_empty() {
            self.files.push((f.path.to_path_buf(), f.offenses.clone()));
        }
    }

    fn finished(&mut self, out: &mut String, cx: &Ctx, _inspected: &[PathBuf]) {
        let min = minimum_to_fail(cx.opts.fail_level, &cx.pwd_config);
        for (file, offs) in &self.files {
            for o in offs {
                let sev = if o.severity < min { "warning" } else { "error" };
                let msg = o.message.replace('%', "%25").replace('\n', "%0A").replace('\r', "%0D");
                // Deliberate difference: RuboCop leaves the file property unescaped, so a file name
                // containing a line break could start a new workflow command. Escape CR/LF there.
                let file = cx.smart_path(file).replace('\n', "%0A").replace('\r', "%0D");
                let _ = write!(out, "\n::{sev} file={},line={},col={}::{msg}", file, o.line, o.real_column());
            }
        }
        out.push('\n');
    }
}

/// `-f json`
#[derive(Default)]
pub(super) struct Json {
    files: Vec<String>,
    offense_count: usize,
    target_count: usize,
    error_count: usize,
    warning_count: usize,
}

impl Formatter for Json {
    fn started(&mut self, _out: &mut String, _cx: &Ctx, targets: &[PathBuf]) -> Result<(), String> {
        self.target_count = targets.len();
        Ok(())
    }

    fn file_finished(&mut self, _out: &mut String, cx: &Ctx, f: &FileOut) {
        let offs: Vec<String> = f.offenses.iter().map(|o| hash_for_offense(o, f.config)).collect();
        let path = cx.smart_path(f.path);
        let (problems, errors, warnings) = hash_for_problems(&path, f.path, f.errors, f.warnings);
        self.error_count += errors;
        self.warning_count += warnings;
        self.files.push(format!("{{\"path\":{},\"offenses\":[{}]{problems}}}", json(&path), offs.join(",")));
        self.offense_count += f.offenses.len();
    }

    fn finished(&mut self, out: &mut String, _cx: &Ctx, inspected: &[PathBuf]) {
        let _ = write!(
            out,
            "{{\"metadata\":{{\"rubocop_version\":{},\"ruby_engine\":\"ruby\",\"ruby_version\":{},\
             \"ruby_patchlevel\":{},\"ruby_platform\":{}}},\"files\":[{}],\"summary\":{{\"offense_count\":{},\
             \"target_file_count\":{},\"inspected_file_count\":{}{}}}}}",
            json(RUBOCOP_VERSION),
            json(RUBY_VERSION),
            json(RUBY_PATCHLEVEL),
            json(&ruby_platform()),
            self.files.join(","),
            self.offense_count,
            self.target_count,
            inspected.len(),
            problem_counts(self.error_count, self.warning_count)
        );
    }
}

/// JSONFormatter#hash_for_offense, as a JSON object (`correction` included when the offense has
/// corrections; `config` is the file's, for Offense#correction_safe).
pub fn hash_for_offense(o: &Off, config: &crate::config::Resolved) -> String {
    let mut s = format!(
        "{{\"severity\":{},\"message\":{},\"cop_name\":{},\"corrected\":{},\"correctable\":{},\
                     \"location\":{{\"start_line\":{},\"start_column\":{},\"last_line\":{},\"last_column\":{},\
                     \"length\":{},\"line\":{},\"column\":{}}}",
        json(o.severity.name()),
        json(&o.message),
        json(o.cop),
        o.status.corrected(),
        o.status.correctable(),
        o.line,
        o.real_column(),
        o.last_line,
        o.real_last_column(),
        o.length,
        o.line,
        o.real_column()
    );
    if o.status.disabled() {
        let j = o.justification.as_deref().map_or_else(|| "null".to_string(), json);
        s.push_str(&format!(",\"suppressed\":true,\"justification\":{j}"));
    }
    if !o.corrections.is_empty() {
        let edits: Vec<String> = o
            .corrections
            .iter()
            .map(|e| {
                format!(
                    "{{\"start_line\":{},\"start_column\":{},\"last_line\":{},\"last_column\":{},\
                                 \"begin_pos\":{},\"end_pos\":{},\"replacement\":{}}}",
                    e.start_line,
                    e.start_column,
                    e.last_line,
                    e.last_column,
                    e.begin_pos,
                    e.end_pos,
                    json(&e.replacement)
                )
            })
            .collect();
        // Offense#correction_safe: Cop::Base#safe_autocorrect? with the file's config.
        let c = config.hash.map(o.cop).cloned().unwrap_or_default();
        let safe = c.bool("Safe") != Some(false) && c.bool("SafeAutoCorrect") != Some(false);
        s.push_str(&format!(",\"correction\":{{\"safe\":{safe},\"edits\":[{}]}}", edits.join(",")));
    }
    s.push('}');
    s
}

/// JSONFormatter#problem_counts: the summary's `,"error_count":N,"warning_count":N`, each only
/// when non-zero.
pub fn problem_counts(errors: usize, warnings: usize) -> String {
    [("error_count", errors), ("warning_count", warnings)]
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| format!(",\"{k}\":{n}"))
        .collect()
}

/// JSONFormatter#hash_for_problems: the `,"errors":[...]` and `,"warnings":[...]` members of a
/// file's entry (each only when non-empty) and how many errors and warnings they list. Messages
/// that differ only in the line and column after the file's absolute path are listed once, naming
/// the file as `path` (the entry's smart path) does.
pub fn hash_for_problems(path: &str, file: &Path, errors: &[String], warnings: &[String]) -> (String, usize, usize) {
    let mut out = String::new();
    let mut counts = [0, 0];
    if errors.is_empty() && warnings.is_empty() {
        return (out, 0, 0);
    }
    let abs = crate::config::expand_path(file, &crate::config::cwd()).to_string_lossy().into_owned();
    let location = regex::Regex::new(&format!("{}((?::\\d+)*)", regex::escape(&abs))).unwrap();
    for (i, (key, messages)) in [("errors", errors), ("warnings", warnings)].into_iter().enumerate() {
        if messages.is_empty() {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        let listed: Vec<String> = messages
            .iter()
            .filter(|m| seen.insert(location.replace_all(m, "").into_owned()))
            .map(|m| json(&location.replace_all(m, |c: &regex::Captures| format!("{path}{}", &c[1]))))
            .collect();
        counts[i] = listed.len();
        out.push_str(&format!(",\"{key}\":[{}]", listed.join(",")));
    }
    (out, counts[0], counts[1])
}

/// A JUnit testcase: classname, cop, failures as (message, location).
type Case = (String, &'static str, Vec<(String, String)>);

/// `-f junit`
#[derive(Default)]
pub(super) struct Junit {
    cases: Vec<Case>,
    files: usize,
    offenses: usize,
    classnames: HashMap<PathBuf, String>,
}

/// Offense::NO_LOCATION#to_s (Struct#inspect).
const PSEUDO_SOURCE_RANGE: &str = "#<struct RuboCop::Cop::Offense::PseudoSourceRange line=1, column=0, \
                                   source_line=\"\", begin_pos=0, end_pos=0>";

fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            c => out.push(c),
        }
    }
    out
}

/// The cops RuboCop's registry knows that rbcop can report (its own plus Lint/Syntax), in registry
/// (load) order as Registry#enabled lists them.
fn known_cops() -> Vec<&'static str> {
    let mut v: Vec<&'static str> =
        crate::cops::ALL.iter().map(|r| r.name).filter(|n| crate::helpers::cop_registry::is_registered(n)).collect();
    v.push("Lint/Syntax");
    v.sort_by_cached_key(|c| crate::autocorrect::registry_index(c));
    v
}

fn name_matches(list: &[String], cop: &str) -> bool {
    list.iter().any(|n| n == cop || cop.rsplit_once('/').is_some_and(|(d, _)| d == n))
}

impl Formatter for Junit {
    fn file_finished(&mut self, _out: &mut String, cx: &Ctx, f: &FileOut) {
        self.files += 1;
        self.offenses += f.offenses.len();
        let only = cx.opts.only.as_deref();
        for cop in known_cops() {
            if name_matches(&cx.opts.except, cop) || only.is_some_and(|o| !name_matches(o, cop)) {
                continue;
            }
            // Registry#enabled: --only names are enabled outright, others by config.
            if !f.config.cop_enabled(cop) {
                continue;
            }
            let target: Vec<&Off> = f.offenses.iter().filter(|o| o.cop == cop).collect();
            if cx.opts.display_only_failed && target.is_empty() {
                continue;
            }
            let pwd = format!("{}/", cx.pwd);
            let classname = self
                .classnames
                .entry(f.path.to_path_buf())
                .or_insert_with(|| {
                    let p = f.path.display().to_string();
                    p.strip_suffix(".rb").unwrap_or(&p).replace(&pwd, "").replace('/', ".")
                })
                .clone();
            let failures = target
                .iter()
                .map(|o| {
                    // `offense.location.to_s`: a global offense's PseudoSourceRange is a Struct, so
                    // its #inspect.
                    let text = if o.global {
                        PSEUDO_SOURCE_RANGE.to_string()
                    } else {
                        format!("{}:{}:{}", f.path.display(), o.line, o.real_column())
                    };
                    (o.message.clone(), text)
                })
                .collect();
            self.cases.push((classname, cop, failures));
        }
    }

    fn finished(&mut self, out: &mut String, _cx: &Ctx, _inspected: &[PathBuf]) {
        out.push_str("<?xml version='1.0'?>\n<testsuites>\n");
        let _ = writeln!(out, "  <testsuite name='rubocop' tests='{}' failures='{}'>", self.files, self.offenses);
        for (class, cop, failures) in &self.cases {
            if failures.is_empty() {
                let _ = writeln!(out, "    <testcase classname='{}' name='{}'/>", xml(class), xml(cop));
                continue;
            }
            let _ = writeln!(out, "    <testcase classname='{}' name='{}'>", xml(class), xml(cop));
            for (message, text) in failures {
                let _ = writeln!(out, "      <failure type='{}' message='{}'>", xml(cop), xml(message));
                let _ = writeln!(out, "        {}", xml(text));
                out.push_str("      </failure>\n");
            }
            out.push_str("    </testcase>\n");
        }
        out.push_str("  </testsuite>\n</testsuites>\n");
    }
}

/// `-f markdown`
#[derive(Default)]
pub(super) struct Markdown {
    files: Vec<(PathBuf, Vec<Off>)>,
    offenses: usize,
}

impl Formatter for Markdown {
    fn file_finished(&mut self, _out: &mut String, _cx: &Ctx, f: &FileOut) {
        self.offenses += f.offenses.len();
        self.files.push((f.path.to_path_buf(), f.offenses.clone()));
    }

    fn finished(&mut self, out: &mut String, cx: &Ctx, inspected: &[PathBuf]) {
        out.push_str("# RuboCop Inspection Report\n\n");
        let _ = write!(
            out,
            "{} inspected, {} detected:\n\n",
            pluralize(inspected.len(), "file", false),
            pluralize(self.offenses, "offense", true)
        );
        for (path, offs) in self.files.iter().filter(|(_, o)| !o.is_empty()) {
            let _ = writeln!(out, "### {} - ({})", cx.relative(path), pluralize(offs.len(), "offense", false));
            for o in offs {
                let _ = write!(out, "  * **Line # {} - {}:** {}\n\n", o.line, o.severity.name(), o.message);
                let code = format!("{}{}", o.source_line, if o.single_line() { "" } else { " ..." });
                if !blank(&code) {
                    let _ = write!(out, "    ```rb\n    {code}\n    ```\n\n");
                }
            }
        }
    }
}

/// `-f html`
#[derive(Default)]
pub(super) struct Html {
    files: Vec<(PathBuf, Vec<Off>)>,
    offenses: usize,
}

impl Formatter for Html {
    fn file_finished(&mut self, _out: &mut String, _cx: &Ctx, f: &FileOut) {
        self.offenses += f.offenses.len();
        self.files.push((f.path.to_path_buf(), f.offenses.clone()));
    }

    /// output.html.erb, rendered the way ERB lays out its tags (text around `<% %>` kept), then
    /// whitespace-only lines reduced to "\n" as HTMLFormatter#render_html does.
    fn finished(&mut self, out: &mut String, cx: &Ctx, _inspected: &[PathBuf]) {
        let mut files: Vec<&(PathBuf, Vec<Off>)> = self.files.iter().collect();
        files.sort_by(|a, b| a.0.as_os_str().cmp(b.0.as_os_str()));
        let mut s = String::from(include_str!("html_head.html"));
        let _ = write!(
            s,
            "          {} inspected,\n          {} detected:\n        </div>\n        <ul class=\"offenses-list\">\n          ",
            pluralize(files.len(), "file", false),
            pluralize(self.offenses, "offense", true)
        );
        for (path, offs) in &files {
            s.push_str("\n            ");
            if offs.is_empty() {
                continue;
            }
            let rel = html(&cx.relative(path));
            let _ = write!(
                s,
                "\n            <li>\n              <a href=\"#offense_{rel}\">\n                {rel} - {}\n              </a>\n            </li>\n          ",
                pluralize(offs.len(), "offense", false)
            );
        }
        s.push_str("\n        </ul>\n      </div>\n    </div>\n    <div id=\"offenses\">\n      ");
        for (path, offs) in &files {
            s.push_str("\n      ");
            if !offs.is_empty() {
                let rel = html(&cx.relative(path));
                let _ = write!(
                    s,
                    "\n      <div class=\"offense-box\" id=\"offense_{rel}\">\n        <div class=\"box-title-placeholder\"><h3>&nbsp;</h3></div>\n        <div class=\"box-title\"><h3>{rel} - {}</h3></div>\n        <div class=\"offense-reports\">\n          ",
                    pluralize(offs.len(), "offense", false)
                );
                for o in offs {
                    let sev = o.severity.name();
                    // Code spans are escaped like RuboCop; the text around them is escaped too (see html_text).
                    let msg = sub_backticks(&html_text(&o.message), false, 1, |c| {
                        format!("<code>{}</code>", html(&unescape_text(c)))
                    });
                    let _ = write!(
                        s,
                        "\n          <div class=\"report\">\n            <div class=\"meta\">\n              <span class=\"location\">Line #{}</span> –\n              <span class=\"severity {sev}\">{sev}:</span>\n              <span class=\"message\">{msg}</span>\n            </div>\n            ",
                        o.line
                    );
                    if !o.source_line.trim_matches(|c| " \t\n\x0b\x0c\r\0".contains(c)).is_empty() {
                        let (b, e) = o.highlight();
                        let line: Vec<char> = o.source_line.chars().collect();
                        let before: String = line[..b].iter().collect();
                        let after: String = line[e..].iter().collect();
                        let ell = if o.single_line() { "" } else { " <span class=\"extra-code\">...</span>" };
                        let _ = write!(
                            s,
                            "\n            <pre><code>{}<span class=\"highlight {sev}\">{}</span>{}{ell}</code></pre>\n            ",
                            html(&before),
                            html(&o.highlighted()),
                            html(&after)
                        );
                    }
                    s.push_str("\n          </div>\n          ");
                }
                s.push_str("\n        </div>\n      </div>\n      ");
            }
            s.push_str("\n      ");
        }
        let _ = write!(
            s,
            "\n    </div>\n    <footer>\n      Generated by <a href=\"https://github.com/rubocop/rubocop\">RuboCop</a>\n      <span class=\"version\">{RUBOCOP_VERSION}</span>\n    </footer>\n  </body>\n</html>\n"
        );
        for line in s.split_inclusive('\n') {
            out.push_str(if line.trim().is_empty() { "\n" } else { line });
        }
    }
}

/// `-f offenses`: offense counts per cop.
pub(super) struct Offenses {
    counts: HashMap<&'static str, usize>,
    links: HashMap<&'static str, Option<String>>,
    files: usize,
    bar: Option<Bar>,
    tty: bool,
    global: Paint,
}

impl Offenses {
    pub(super) fn new(tty: bool, global: Paint) -> Offenses {
        Offenses { counts: HashMap::new(), links: HashMap::new(), files: 0, bar: None, tty, global }
    }
}

impl Formatter for Offenses {
    fn started(&mut self, out: &mut String, _cx: &Ctx, targets: &[PathBuf]) -> Result<(), String> {
        if self.tty {
            let mut bar = Bar::new(true, targets.len());
            bar.refresh(out);
            self.bar = Some(bar);
        }
        Ok(())
    }

    fn file_finished(&mut self, out: &mut String, cx: &Ctx, f: &FileOut) {
        for o in &f.offenses {
            *self.counts.entry(o.cop).or_default() += 1;
        }
        if cx.opts.display_style_guide {
            let re = regex::Regex::new(r" \(http\S+\)\n?\z").unwrap();
            for o in &f.offenses {
                let link = self.links.entry(o.cop).or_default();
                if link.is_none() {
                    *link = re.find(&o.message).map(|m| m.as_str().trim_end_matches('\n').to_string());
                }
            }
        }
        if !f.offenses.is_empty() {
            self.files += 1;
        }
        if let Some(bar) = self.bar.as_mut() {
            bar.increment(out);
        }
    }

    fn finished(&mut self, out: &mut String, cx: &Ctx, _inspected: &[PathBuf]) {
        let mut counts: Vec<(&str, usize)> = self.counts.iter().map(|(k, v)| (*k, *v)).collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let total: usize = counts.iter().map(|c| c.1).sum();
        let width = total.to_string().len() + 2;
        out.push('\n');
        for (cop, n) in counts {
            let correctable = if cx.supports_autocorrect(cop) {
                let safety = if cx.safe_autocorrect(cop) { "Safe" } else { "Unsafe" };
                self.global.yellow(&format!(" [{safety} Correctable]"))
            } else {
                String::new()
            };
            let link = self.links.get(cop).cloned().flatten().unwrap_or_default();
            let _ = writeln!(out, "{:<width$}{cop}{correctable}{link}", n.to_string());
        }
        let _ = write!(out, "--\n{total}  Total in {}\n\n", pluralize(self.files, "file", false));
    }
}

/// `-f worst`: offense counts per file.
#[derive(Default)]
pub(super) struct Worst {
    counts: Vec<(String, usize)>,
}

impl Formatter for Worst {
    fn file_finished(&mut self, _out: &mut String, cx: &Ctx, f: &FileOut) {
        if !f.offenses.is_empty() {
            self.counts.push((relative_path(f.path, Path::new(&cx.pwd)), f.offenses.len()));
        }
    }

    fn finished(&mut self, out: &mut String, _cx: &Ctx, _inspected: &[PathBuf]) {
        // Pathname#<=> compares with '/' sorting first.
        let key = |s: &str| s.replace('/', "\0");
        self.counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| key(&a.0).cmp(&key(&b.0))));
        let total: usize = self.counts.iter().map(|c| c.1).sum();
        let width = total.to_string().len() + 2;
        out.push('\n');
        for (file, n) in &self.counts {
            let _ = writeln!(out, "{:<width$}{file}", n.to_string());
        }
        let _ = write!(out, "--\n{total}  Total in {}\n\n", pluralize(self.counts.len(), "file", false));
    }
}

/// `-f sarif`
#[derive(Default)]
pub(super) struct Sarif {
    rules: Vec<String>,
    index: HashMap<&'static str, usize>,
    results: Vec<String>,
}

const SARIF_LEVELS: &[(&str, &str)] = &[
    ("info", "note"),
    ("refactor", "note"),
    ("convention", "note"),
    ("warning", "warning"),
    ("error", "error"),
    ("fatal", "error"),
];

fn sarif_level(sev: &str) -> &'static str {
    SARIF_LEVELS.iter().find(|(s, _)| *s == sev).map_or("warning", |(_, l)| l)
}

impl Formatter for Sarif {
    fn file_finished(&mut self, _out: &mut String, cx: &Ctx, f: &FileOut) {
        // fingerprints_for: a repeat of a line hash within the file gets a `:N` suffix.
        let mut line_hash_counts: HashMap<String, usize> = HashMap::new();
        for o in &f.offenses {
            let index = match self.index.get(o.cop) {
                Some(&i) => i,
                None => {
                    self.rules.push(sarif_rule(f, o.cop));
                    self.index.insert(o.cop, self.rules.len() - 1);
                    self.rules.len() - 1
                }
            };
            let line_hash = sha256_hex(format!("{}{}", o.cop, o.source_line).as_bytes());
            let count = line_hash_counts.entry(line_hash.clone()).or_default();
            *count += 1;
            let hash = if *count == 1 { line_hash } else { format!("{line_hash}:{count}") };
            let mut r = format!(
                "{{\"ruleId\":{},\"ruleIndex\":{index},\"level\":\"{}\",\"message\":{{\"text\":{}}},\
                 \"locations\":[{{\"physicalLocation\":{{\"artifactLocation\":{{\"uri\":{}}},\"region\":{{\
                 \"startLine\":{},\"startColumn\":{},\"endLine\":{},\"endColumn\":{}}}}}}}],\
                 \"partialFingerprints\":{{\"primaryLocationLineHash\":\"{hash}\"}}",
                json(o.cop),
                sarif_level(o.severity.name()),
                json(&o.message),
                json(&cx.smart_path(f.path).replace('\\', "/")),
                o.line,
                o.real_column(),
                o.last_line,
                o.real_last_column() + 1
            );
            if o.status.disabled() {
                r.push_str(",\"suppressions\":[{\"kind\":\"inSource\"");
                if let Some(j) = &o.justification {
                    r.push_str(&format!(",\"justification\":{}", json(j)));
                }
                r.push_str("}]");
            }
            r.push('}');
            self.results.push(r);
        }
    }

    fn finished(&mut self, out: &mut String, _cx: &Ctx, _inspected: &[PathBuf]) {
        let _ = write!(
            out,
            "{{\"$schema\":\"https://json.schemastore.org/sarif-2.1.0.json\",\"version\":\"2.1.0\",\"runs\":[{{\
             \"tool\":{{\"driver\":{{\"name\":\"RuboCop\",\"version\":\"{v}\",\"semanticVersion\":\"{v}\",\
             \"informationUri\":\"https://rubocop.org\",\"rules\":[{}]}}}},\"results\":[{}]}}]}}",
            self.rules.join(","),
            self.results.join(","),
            v = RUBOCOP_VERSION
        );
    }
}

/// SARIFFormatter#rule_for
fn sarif_rule(f: &FileOut, cop: &str) -> String {
    let cfg = f.config.hash.map(cop).cloned().unwrap_or_default();
    let desc = cfg.str("Description").unwrap_or(cop).to_string();
    let dept = cop.rsplit_once('/').map_or("", |(d, _)| d);
    let uri = super::docs_url(f.config, cop);
    // Cop::Base::DEPARTMENT_SEVERITIES
    let default_sev = match dept {
        "Lint" | "Security" => "warning",
        "Metrics" => "refactor",
        _ => "convention",
    };
    let level = sarif_level(cfg.str("Severity").unwrap_or(default_sev));
    let tag = cop.split('/').next().unwrap_or(cop);
    // help_for / rule[:helpUri]: both only with a documentation URL.
    let (markdown, help_uri) = match &uri {
        Some(uri) => (
            format!(",\"markdown\":{}", json(&format!("{desc}\n\n[Documentation]({uri})"))),
            format!(",\"helpUri\":{}", json(uri)),
        ),
        None => (String::new(), String::new()),
    };
    format!(
        "{{\"id\":{c},\"name\":{c},\"shortDescription\":{{\"text\":{d}}},\"fullDescription\":{{\"text\":{d}}},\
         \"help\":{{\"text\":{d}{markdown}}},\"defaultConfiguration\":{{\"level\":\"{level}\"}},\
         \"properties\":{{\"tags\":[{}]}}{help_uri}}}",
        json(tag),
        c = json(cop),
        d = json(&desc)
    )
}

/// SHA-256 as lowercase hex (Digest::SHA256.hexdigest).
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha256() {
        assert_eq!(super::sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(super::sha256_hex(&[b'a'; 64]), "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb");
        // RuboCop's SARIF fingerprint for a global Bundler/GemFilename offense (empty source line).
        assert_eq!(
            super::sha256_hex(b"Bundler/GemFilename"),
            "11c77c0aa793d0bf956c071f6d0beeed64a5d07340f2c8b8fdc820611139bab4"
        );
    }
}
