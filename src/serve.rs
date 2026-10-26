//! What `--lsp` and `--mcp` share: RuboCop's LSP::Runtime (a buffer inspected or formatted as a
//! path, its offenses as LSP diagnostics), the URI helpers and the JSON both protocols speak.

use crate::Badges;
use rbcop::autocorrect::Mode;
use rbcop::config::{Resolved, Store, expand_path, relative_path};
use rbcop::formatter::{self, Annotate, Off, json};
use rbcop::runner::{self, FileReport};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// LSP::Runtime over its StdinRunner. Plugin cops rbcop does not implement are not run here, and
/// the RuboCop fallback never is (RBCOP_FALLBACK or not).
pub struct Runtime {
    pub store: Store,
    pub cwd: PathBuf,
    /// StdinRunner#config_for_working_directory: documentation URLs come from it.
    pwd_config: Arc<Resolved>,
    /// Runtime#config_only_options: `Lint` (lintMode) and `Layout` (layoutMode).
    pub only: Vec<String>,
    /// The `--only` each config's cops were first mobilized with, by config.
    mobilized: HashMap<usize, Vec<String>>,
}

/// What a run returns: the report and, when autocorrecting, the corrected source.
struct Run {
    report: FileReport,
    corrected: Option<String>,
    /// LSP::DisableCommentEdits' multiline literal ranges; None without a tree.
    literals: Option<Vec<(usize, usize)>>,
    /// The file's config (its MessageAnnotator settings).
    cfg: Arc<Resolved>,
}

impl Runtime {
    /// LSP::Runtime.new: serving, with LSP enabled.
    pub fn new(store: Store, pwd_config: Arc<Resolved>) -> Runtime {
        rbcop::helpers::lsp::start_serving();
        let cwd = store.cwd().to_path_buf();
        Runtime { store, cwd, pwd_config, only: Vec::new(), mobilized: HashMap::new() }
    }

    /// Runtime#offenses: `text` inspected as `path`, each offense an LSP diagnostic.
    pub fn offenses(&mut self, path: &str, text: String) -> Result<Vec<String>, String> {
        let run = self.run(path, text, None)?;
        self.diagnostics(path, &run)
    }

    /// Runtime#format: `text` autocorrected as `path` (safe corrections only when `safe`).
    pub fn format(&mut self, path: &str, text: String, safe: bool) -> Result<String, String> {
        let run = self.run(path, text.clone(), Some(if safe { Mode::Safe } else { Mode::All }))?;
        Ok(run.corrected.unwrap_or(text))
    }

    /// StdinRunner#run: the buffer inspected (or autocorrected) as `path`, with the
    /// `--force-exclusion` of `-s` and the `--only` of lintMode/layoutMode.
    fn run(&mut self, path: &str, text: String, mode: Option<Mode>) -> Result<Run, String> {
        let abs = expand_path(std::path::Path::new(path), &self.cwd);
        let skipped = |text: String| Run {
            report: FileReport { offenses: Vec::new(), text, warnings: Vec::new(), abort: None, diffs: Vec::new() },
            corrected: None,
            literals: None,
            cfg: self.pwd_config.clone(),
        };
        if self.store.for_dir(&self.cwd)?.file_to_exclude(&abs) {
            return Ok(skipped(text));
        }
        let cfg = self.store.for_file(&abs)?;
        // Runner#mobilized_cop_classes is memoized per config: the first run's --only sticks to it
        // (a later `initialize` changing lintMode/layoutMode does not reach that config's files).
        let only = self.mobilized.entry(Arc::as_ptr(&cfg) as usize).or_insert_with(|| self.only.clone()).clone();
        let badges = Badges::new(&only, &[], false);
        cfg.mobilize_team();
        rbcop::config::validator::cop_validate_config(&cfg, |cop| {
            badges.badge_selected(cop, &cfg) && cfg.cop_enabled(cop)
        })?;
        let plugin_file = self.store.copless_plugin_file(&cfg, &abs);
        let guide_only = badges.guide_only(&cfg);
        let selected = |cop: &str| !plugin_file && badges.badge_selected_in(cop, &cfg, guide_only);
        let display = relative_path(&abs, &self.cwd);
        rbcop::config::value::reset_run_budget();
        runner::take_literal_ranges();
        let linted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match mode {
            None => (runner::inspect(&abs, &display, text, &cfg, &selected), None, None),
            Some(mode) => {
                let c = rbcop::autocorrect::correct_text(&abs, &display, text, &cfg, &selected, mode);
                (c.report, c.source, c.error)
            }
        }));
        let literals = runner::take_literal_ranges();
        for w in runner::take_file_warnings().into_iter().chain(runner::take_file_errors()) {
            eprintln!("{w}");
        }
        let errored = runner::take_errored();
        rbcop::autogen::take_file();
        let (report, corrected, loop_error) =
            linted.map_err(|_| format!("rbcop: internal error while inspecting {}", abs.display()))?;
        for w in &report.warnings {
            eprintln!("{w}");
        }
        // StdinRunner raises cop errors (`raise_cop_error: true`) and infinite loops: no response.
        if let Some(e) = loop_error {
            return Err(format!("RuboCop::Runner::InfiniteCorrectionLoop {e}"));
        }
        if errored {
            return Err(format!("RuboCop::ErrorWithAnalyzedFileLocation inspecting {display}"));
        }
        Ok(Run { report, corrected, literals, cfg })
    }

    /// Runtime#offenses: each offense as LSP::Diagnostic#to_lsp_diagnostic writes it.
    fn diagnostics(&self, path: &str, run: &Run) -> Result<Vec<String>, String> {
        let text = &run.report.text;
        let offenses = formatter::offenses(text, &run.report.offenses, |o| {
            formatter::annotate(&run.cfg, o.cop, &o.message, Annotate::default())
        });
        if offenses.is_empty() {
            return Ok(Vec::new());
        }
        let uri = ensure_uri_scheme(path)?;
        let starts = formatter::line_starts(text);
        let line_of = |off: usize| starts.partition_point(|&s| s <= off.min(text.len()));
        let line_text = |line: usize| {
            let s = starts[line - 1];
            text[s..].split('\n').next().unwrap_or("")
        };
        let position = |line: usize, character: usize| format!("{{\"line\":{line},\"character\":{character}}}");
        let edit = |start: String, end: String, new: &str| {
            format!("{{\"range\":{{\"start\":{start},\"end\":{end}}},\"newText\":{}}}", json(new))
        };
        let document_edit = |edits: Vec<String>| {
            format!(
                "{{\"documentChanges\":[{{\"textDocument\":{{\"version\":null,\"uri\":{}}},\"edits\":[{}]}}]}}",
                json(&uri),
                edits.join(",")
            )
        };
        let mut out = Vec::new();
        for o in &offenses {
            let line = &o.source_line;
            let correctable = o.edits.is_some();
            let mut actions = Vec::new();
            if let Some(edits) = &o.edits {
                // Diagnostic#offense_replacements: characters counted on the offense's own line.
                let at = |off: usize| {
                    let (l, c) = formatter::line_col_at(text, &starts, off);
                    position(l - 1, utf16(line, c - 1))
                };
                let edits = edits.iter().map(|(s, e, new)| edit(at(*s), at(*e), new)).collect();
                actions.push(format!(
                    "{{\"title\":{},\"kind\":\"quickfix\",\"isPreferred\":true,\"edit\":{}}}",
                    json(&format!("Autocorrect {}", o.cop)),
                    document_edit(edits)
                ));
            }
            let literal = run
                .literals
                .as_ref()
                .and_then(|ranges| ranges.iter().find(|&&(s, e)| o.line >= line_of(s) && o.line < line_of(e.max(s))));
            let disable = match literal {
                // DisableCommentEdits#block_disable_comments: around the literal's whole lines.
                Some(&(s, e)) => {
                    let (first, last) = (line_of(s), line_of(e.max(s)));
                    let indent: &str = {
                        let t = line_text(first);
                        &t[..t.len() - t.trim_start_matches([' ', '\t', '\r', '\n', '\x0c', '\x0b']).len()]
                    };
                    vec![
                        edit(
                            position(first - 1, 0),
                            position(first - 1, 0),
                            &format!("{indent}# rubocop:disable {}\n", o.cop),
                        ),
                        edit(
                            position(last - 1, utf16(line_text(last), usize::MAX)),
                            position(last - 1, utf16(line_text(last), usize::MAX)),
                            &format!("\n{indent}# rubocop:enable {}", o.cop),
                        ),
                    ]
                }
                None => {
                    let eol = position(o.line - 1, utf16(line, usize::MAX));
                    let comment = if line.contains(" # rubocop:disable ") {
                        format!(",{}", o.cop)
                    } else {
                        format!(" # rubocop:disable {}", o.cop)
                    };
                    vec![edit(eol.clone(), eol, &comment)]
                }
            };
            actions.push(format!(
                "{{\"title\":{},\"kind\":\"quickfix\",\"edit\":{}}}",
                json(&format!("Disable {} for this line", o.cop)),
                document_edit(disable)
            ));
            out.push(diagnostic(o, correctable, formatter::docs_url(&self.pwd_config, o.cop), &actions));
        }
        Ok(out)
    }
}

/// LSP::Diagnostic#to_lsp_diagnostic, in LanguageServer::Protocol::Interface::Diagnostic's key order.
fn diagnostic(o: &Off, correctable: bool, docs: Option<String>, actions: &[String]) -> String {
    let (begin, end) = o.highlight();
    let line = o.line - 1;
    let severity = match o.severity.name() {
        "fatal" | "error" => 1,
        "warning" => 2,
        "convention" => 3,
        _ => 4,
    };
    let mut message = o.message.clone();
    if !correctable {
        message.push_str("\n\nThis offense is not autocorrectable.\n");
    }
    let docs = docs.map_or(String::new(), |href| format!(",\"codeDescription\":{{\"href\":{}}}", json(&href)));
    format!(
        "{{\"range\":{{\"start\":{{\"line\":{line},\"character\":{}}},\"end\":{{\"line\":{line},\"character\":{}}}}},\
         \"severity\":{severity},\"code\":{}{docs},\"source\":\"RuboCop\",\"message\":{},\
         \"data\":{{\"correctable\":{correctable},\"code_actions\":[{}]}}}}",
        utf16(&o.source_line, begin),
        utf16(&o.source_line, end),
        json(o.cop),
        json(&message),
        actions.join(",")
    )
}

/// Diagnostic#to_position_character: the UTF-16 length of the first `chars` characters of `line`.
fn utf16(line: &str, chars: usize) -> usize {
    line.chars().take(chars).map(char::len_utf16).sum()
}

/// Routes#convert_file_uri_to_path: `URI.decode_www_form_component(uri.delete_prefix('file://'))`.
pub fn uri_path(uri: &str) -> Result<String, String> {
    let s = uri.strip_prefix("file://").unwrap_or(uri).as_bytes();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let byte = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
                out.push(byte.ok_or_else(|| format!("ArgumentError invalid %-encoding ({uri})"))?);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Diagnostic#ensure_uri_scheme: `URI.parse(path)`, scheme `file` when it has none.
fn ensure_uri_scheme(path: &str) -> Result<String, String> {
    // URI::RFC3986_Parser: anything outside these characters (a space, non-ASCII) or a stray `%`
    // is an InvalidURIError.
    let b = path.as_bytes();
    let valid = b.iter().enumerate().all(|(i, &c)| {
        c.is_ascii_alphanumeric()
            || b"-._~!$&'()*+,;=:@/?#".contains(&c)
            || (c == b'%' && b.get(i + 1..i + 3).is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)))
    }) && path.matches('#').count() <= 1;
    if !valid {
        return Err(format!("URI::InvalidURIError bad URI (is not URI?): {path:?}"));
    }
    let scheme = path.split_once(':').is_some_and(|(s, _)| {
        s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    Ok(if scheme { path.to_string() } else { format!("file://{path}") })
}

/// A parsed JSON value (`JSON.parse(message, symbolize_names: true)`), keys in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The literal as written.
    // ponytail: echoed as written; Ruby would print a float as `Float#to_s` (`1e2` as `100.0`).
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn dig(&self, keys: &[&str]) -> Option<&Json> {
        keys.iter().try_fold(self, |j, k| j.get(k))
    }

    pub fn items(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => &[],
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Json::Num(n) => n.parse().ok(),
            _ => None,
        }
    }

    /// A string's value; any other value as `to_s` would print it.
    pub fn string(&self) -> String {
        match self {
            Json::Str(s) => s.clone(),
            Json::Null => String::new(),
            other => other.text(),
        }
    }

    /// JSON.generate
    pub fn text(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(b) => b.to_string(),
            Json::Num(n) => n.clone(),
            Json::Str(s) => json(s),
            Json::Arr(a) => format!("[{}]", a.iter().map(Json::text).collect::<Vec<_>>().join(",")),
            Json::Obj(m) => format!(
                "{{{}}}",
                m.iter().map(|(k, v)| format!("{}:{}", json(k), v.text())).collect::<Vec<_>>().join(",")
            ),
        }
    }

    pub fn parse(s: &str) -> Result<Json, String> {
        let mut p = Parser { s: s.as_bytes(), i: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("unexpected character at {}", p.i));
        }
        Ok(v)
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    /// The four hex digits of a `\u` escape.
    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.s.get(self.i..self.i + 4).ok_or_else(|| format!("incomplete unicode escape at {}", self.i))?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(h).map_err(|e| e.to_string())?, 16).map_err(|e| e.to_string())
    }

    fn eat(&mut self, lit: &str) -> bool {
        let ok = self.s[self.i..].starts_with(lit.as_bytes());
        if ok {
            self.i += lit.len();
        }
        ok
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        let err = |i: usize| format!("unexpected token at {i}");
        match self.s.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut m: Vec<(String, Json)> = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Ok(Json::Obj(m));
                }
                loop {
                    self.ws();
                    let Json::Str(k) = self.value()? else { return Err(err(self.i)) };
                    self.ws();
                    if !self.eat(":") {
                        return Err(err(self.i));
                    }
                    let v = self.value()?;
                    // Ruby's Hash: a repeated key keeps its place and takes the last value.
                    match m.iter_mut().find(|(key, _)| *key == k) {
                        Some(e) => e.1 = v,
                        None => m.push((k, v)),
                    }
                    self.ws();
                    if self.eat("}") {
                        return Ok(Json::Obj(m));
                    }
                    if !self.eat(",") {
                        return Err(err(self.i));
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Ok(Json::Arr(a));
                }
                loop {
                    a.push(self.value()?);
                    self.ws();
                    if self.eat("]") {
                        return Ok(Json::Arr(a));
                    }
                    if !self.eat(",") {
                        return Err(err(self.i));
                    }
                }
            }
            Some(b'"') => {
                self.i += 1;
                let mut out = String::new();
                loop {
                    let start = self.i;
                    while self.s.get(self.i).is_some_and(|&c| c != b'"' && c != b'\\') {
                        self.i += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.s[start..self.i]).map_err(|e| e.to_string())?);
                    match self.s.get(self.i) {
                        Some(b'"') => {
                            self.i += 1;
                            return Ok(Json::Str(out));
                        }
                        Some(b'\\') => {
                            let c = *self.s.get(self.i + 1).ok_or_else(|| err(self.i))?;
                            self.i += 2;
                            match c {
                                b'"' | b'\\' | b'/' => out.push(c as char),
                                b'b' => out.push('\x08'),
                                b'f' => out.push('\x0c'),
                                b'n' => out.push('\n'),
                                b'r' => out.push('\r'),
                                b't' => out.push('\t'),
                                b'u' => {
                                    let hi = self.hex4()?;
                                    let code = if (0xD800..0xDC00).contains(&hi) && self.s[self.i..].starts_with(b"\\u")
                                    {
                                        self.i += 2;
                                        let lo = self.hex4()?;
                                        0x10000 + ((hi - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF)
                                    } else {
                                        hi
                                    };
                                    out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                                }
                                _ => return Err(err(self.i)),
                            }
                        }
                        _ => return Err(err(self.i)),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                self.i += 1;
                while self.s.get(self.i).is_some_and(|c| matches!(c, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
                    self.i += 1;
                }
                Ok(Json::Num(String::from_utf8_lossy(&self.s[start..self.i]).into_owned()))
            }
            _ if self.eat("true") => Ok(Json::Bool(true)),
            _ if self.eat("false") => Ok(Json::Bool(false)),
            _ if self.eat("null") => Ok(Json::Null),
            _ => Err(err(self.i)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips_in_order() {
        let s = r#"{"b":[1,-2.5e3,true,null],"a":{"x":"ü😀\n\"/"},"b":0}"#;
        let j = Json::parse(s).unwrap();
        assert_eq!(j.text(), "{\"b\":0,\"a\":{\"x\":\"ü😀\\n\\\"/\"}}");
        assert!(Json::parse("{bad}").is_err());
    }

    #[test]
    fn uris() {
        assert_eq!(uri_path("file:///a%20b+c.rb").unwrap(), "/a b c.rb");
        assert_eq!(ensure_uri_scheme("/a/b.rb").unwrap(), "file:///a/b.rb");
        assert_eq!(ensure_uri_scheme("untitled:Untitled-1").unwrap(), "untitled:Untitled-1");
        assert!(ensure_uri_scheme("/a b.rb").is_err());
        assert_eq!(utf16("a😀b", 2), 3);
    }
}
