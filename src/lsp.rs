//! `--lsp`: RuboCop 1.91.0's language server (lib/rubocop/lsp/*), JSON-RPC over stdin/stdout.
//! Requests are handled one at a time, in order (RuboCop's server is single-threaded); each
//! response is written as RuboCop's `JSON.generate` writes it, `jsonrpc` last.

use crate::Badges;
use rbcop::autocorrect::Mode;
use rbcop::config::{Resolved, Store, expand_path, relative_path};
use rbcop::formatter::{self, Annotate, Off, json};
use rbcop::runner::{self, FileReport};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

/// Routes::CONFIGURATION_FILE_PATTERNS: a change to one of these ends the server.
const CONFIGURATION_FILE_PATTERNS: [&str; 2] = [".rubocop.yml", ".rubocop_todo.yml"];

/// The InitializeResult RuboCop sends (LanguageServer::Protocol's key order).
const CAPABILITIES: &str = "{\"capabilities\":{\"textDocumentSync\":{\"openClose\":true,\"change\":2},\
\"codeActionProvider\":{\"codeActionKinds\":[\"quickfix\"]},\"documentFormattingProvider\":true,\
\"executeCommandProvider\":{\"commands\":[\"rubocop.formatAutocorrects\",\"rubocop.formatAutocorrectsAll\"]}}}";

/// LSP::Logger.log
fn log(message: &str) {
    eprintln!("[server] {message}");
}

/// Server#start's rescue: `Error #{e.class} #{e.message[0..100]}`.
fn log_error(message: &str) {
    log(&format!("Error {}", message.chars().take(101).collect::<String>()));
}

/// Server#start: read requests until stdin ends. Err is what ends the server early (a request it
/// cannot read), which RuboCop's CLI reports with status 2.
pub fn serve(store: Store, pwd_config: Arc<Resolved>) -> Result<u8, String> {
    rbcop::helpers::lsp::start_serving();
    let cwd = store.cwd().to_path_buf();
    let mut server = Server {
        store,
        cwd,
        pwd_config,
        only: Vec::new(),
        safe_autocorrect: true,
        texts: HashMap::new(),
        mobilized: HashMap::new(),
    };
    let mut input = std::io::stdin().lock();
    while let Some(request) = read_request(&mut input)? {
        if let Err(e) = server.handle(&request) {
            log_error(&e);
        }
    }
    Ok(0)
}

/// Transport::Io::Reader#read: `io.gets("\r\n\r\n")`, the Content-Length header, the body.
fn read_request(input: &mut impl BufRead) -> Result<Option<Json>, String> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0u8];
        match input.read(&mut byte).map_err(|e| e.to_string())? {
            0 => break,
            _ => header.push(byte[0]),
        }
    }
    if header.is_empty() {
        return Ok(None);
    }
    // /Content-Length: (\d+)/i
    let lower = String::from_utf8_lossy(&header).to_ascii_lowercase();
    let length = lower.find("content-length: ").and_then(|i| {
        let digits: String = lower[i + 16..].chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<usize>().ok()
    });
    let length = length.ok_or("undefined method '[]' for nil")?;
    let mut body = Vec::new();
    input.take(length as u64).read_to_end(&mut body).map_err(|e| e.to_string())?;
    if body.is_empty() && length > 0 {
        return Err("unhandled exception".into());
    }
    Json::parse(&String::from_utf8_lossy(&body)).map(Some)
}

/// One message: the hash RuboCop writes, without its closing brace (`jsonrpc` is merged in last).
fn write(open: String) {
    let body = format!("{open},\"jsonrpc\":\"2.0\"}}");
    let mut out = std::io::stdout().lock();
    let _ = write!(out, "Content-Length: {}\r\n\r\n{body}", body.len());
    let _ = out.flush();
}

/// Server#stop: exit (status 0), running `then` first (at_exit).
fn stop(then: impl FnOnce()) -> ! {
    then();
    std::process::exit(0)
}

struct Server {
    store: Store,
    cwd: PathBuf,
    /// StdinRunner#config_for_working_directory: documentation URLs come from it.
    pwd_config: Arc<Resolved>,
    /// Runtime#config_only_options: `Lint` (lintMode) and `Layout` (layoutMode).
    only: Vec<String>,
    safe_autocorrect: bool,
    /// Routes' @text_cache: each open document's text, by URI.
    texts: HashMap<String, String>,
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

impl Server {
    /// Server#start's dispatch (Routes#for, handle_unsupported_method, handle_method_missing).
    fn handle(&mut self, request: &Json) -> Result<(), String> {
        let Json::Obj(_) = request else {
            return Err("NoMethodError undefined method 'key?'".into());
        };
        let id = request.get("id").map_or("null".to_string(), Json::text);
        let Some(method) = request.get("method") else {
            if request.get("id").is_some() {
                write(format!("{{\"id\":{id},\"result\":null"));
            }
            return Ok(());
        };
        let method = match method {
            Json::Str(s) => s.clone(),
            Json::Null => String::new(),
            other => other.text(),
        };
        let params = request.get("params");
        let dig = |keys: &[&str]| params.and_then(|p| p.dig(keys));
        match method.as_str() {
            "initialize" => {
                let option = |key: &str| dig(&["initializationOptions", key]);
                self.safe_autocorrect = matches!(option("safeAutocorrect"), None | Some(Json::Null | Json::Bool(true)));
                self.only.clear();
                if matches!(option("lintMode"), Some(Json::Bool(true))) {
                    self.only.push("Lint".into());
                }
                if matches!(option("layoutMode"), Some(Json::Bool(true))) {
                    self.only.push("Layout".into());
                }
                write(format!("{{\"id\":{id},\"result\":{CAPABILITIES}"));
            }
            "initialized" => log(&format!(
                "RuboCop {} language server initialized, PID {}",
                rbcop::config::RUBOCOP_VERSION,
                std::process::id()
            )),
            "shutdown" => {
                log("Client asked to shutdown RuboCop language server.");
                stop(|| {
                    write(format!("{{\"id\":{id},\"result\":null"));
                    log("Exiting...");
                });
            }
            "textDocument/didChange" => {
                let uri = dig(&["textDocument", "uri"]).map_or(String::new(), Json::string);
                let mut text = self.texts.get(&uri).cloned();
                for change in dig(&["contentChanges"]).map_or(&[][..], Json::items) {
                    let new = change.get("text").map_or(String::new(), Json::string);
                    text = Some(change_text(text.as_deref(), new, change.get("range"))?);
                }
                self.diagnostic(&uri, text.unwrap_or_default())?;
            }
            "textDocument/didOpen" => {
                let uri = dig(&["textDocument", "uri"]).map_or(String::new(), Json::string);
                let text = dig(&["textDocument", "text"]).map_or(String::new(), Json::string);
                self.diagnostic(&uri, text)?;
            }
            "textDocument/didClose" => {
                if let Some(uri) = dig(&["textDocument", "uri"]) {
                    self.texts.remove(&uri.string());
                }
            }
            "textDocument/formatting" => {
                let uri = dig(&["textDocument", "uri"]).map_or(String::new(), Json::string);
                let edits = self.format_file(&uri, None)?;
                write(format!("{{\"id\":{id},\"result\":{edits}"));
            }
            "textDocument/codeAction" => {
                // Code actions travel in each diagnostic's `data`; the client hands them back.
                let only = dig(&["context", "only"]).filter(|o| !matches!(o, Json::Null));
                let mut actions = Vec::new();
                for d in dig(&["context", "diagnostics"]).map_or(&[][..], Json::items) {
                    match d.dig(&["data", "code_actions"]) {
                        Some(Json::Arr(list)) => actions.extend(list.iter()),
                        None | Some(Json::Null) => {}
                        Some(other) => actions.push(other),
                    }
                }
                let kept: Vec<String> = actions
                    .into_iter()
                    .filter(|a| only.is_none_or(|o| o.items().iter().any(|k| Some(k) == a.get("kind"))))
                    .map(Json::text)
                    .collect();
                write(format!("{{\"id\":{id},\"result\":[{}]", kept.join(",")));
            }
            "workspace/didChangeConfiguration" => log("Ignoring workspace/didChangeConfiguration"),
            "workspace/didChangeWatchedFiles" => {
                let changed = dig(&["changes"]).map_or(&[][..], Json::items).iter().any(|c| {
                    let uri = c.get("uri").map_or(String::new(), Json::string);
                    CONFIGURATION_FILE_PATTERNS.iter().any(|p| uri.ends_with(p))
                });
                if changed {
                    log("Configuration file changed; restart required");
                    stop(|| {});
                }
                // ponytail: otherwise RuboCop drops its project index; rbcop keeps none.
            }
            "workspace/executeCommand" => {
                let command = dig(&["command"]).map_or(String::new(), Json::string);
                let label = match command.as_str() {
                    "rubocop.formatAutocorrects" => "Format with RuboCop autocorrects",
                    "rubocop.formatAutocorrectsAll" => "Format all with RuboCop autocorrects",
                    _ => {
                        unsupported(&id, &command);
                        return Ok(());
                    }
                };
                // Clients such as Eglot can invoke a command without arguments.
                let uri = match params.and_then(|p| p.get("arguments")).map(Json::items).and_then(|a| a.first()) {
                    Some(arg @ Json::Obj(_)) => arg.get("uri").filter(|u| !matches!(u, Json::Null | Json::Bool(false))),
                    _ => None,
                };
                let Some(uri) = uri.map(Json::string) else {
                    let message = format!("Missing document URI in arguments for {command}");
                    write(format!("{{\"id\":{id},\"error\":{{\"code\":-32602,\"message\":{}}}", json(&message)));
                    log(&message);
                    return Ok(());
                };
                // The command is an intentional user action: autocorrect as outside LSP. (An error
                // leaves LSP disabled, as RuboCop's LSP.disable block does.)
                rbcop::helpers::lsp::set_enabled(false);
                let edits = self.format_file(&uri, Some(&command))?;
                rbcop::helpers::lsp::set_enabled(true);
                write(format!(
                    "{{\"id\":{id},\"method\":\"workspace/applyEdit\",\"params\":{{\"label\":{},\"edit\":{{\"changes\":{{{}:{edits}}}}}}}",
                    json(label),
                    json(&uri)
                ));
            }
            "textDocument/willSave" | "textDocument/didSave" | "$/cancelRequest" | "$/setTrace" => {}
            // Routes#for finds these public methods too.
            "method_missing" => {
                if request.get("id").is_some() {
                    write(format!("{{\"id\":{id},\"result\":null"));
                }
            }
            _ => unsupported(&id, &method),
        }
        Ok(())
    }

    /// Routes#diagnostic: cache the text, publish its offenses.
    fn diagnostic(&mut self, uri: &str, text: String) -> Result<(), String> {
        self.texts.insert(uri.to_string(), text.clone());
        let path = uri_path(uri)?;
        let run = self.run(&path, text, None)?;
        let diagnostics = self.diagnostics(&path, &run)?;
        write(format!(
            "{{\"method\":\"textDocument/publishDiagnostics\",\"params\":{{\"uri\":{},\"diagnostics\":[{}]}}",
            json(uri),
            diagnostics.join(",")
        ));
        Ok(())
    }

    /// Routes#format_file: one edit replacing the whole document, or none.
    fn format_file(&mut self, uri: &str, command: Option<&str>) -> Result<String, String> {
        let Some(text) = self.texts.get(uri).cloned() else {
            log(&format!("Format request arrived before text synchronized; skipping: `{uri}'"));
            return Ok("[]".into());
        };
        let safe = command.map_or(self.safe_autocorrect, |c| c == "rubocop.formatAutocorrects");
        let path = uri_path(uri)?;
        let run = self.run(&path, text.clone(), Some(if safe { Mode::Safe } else { Mode::All }))?;
        let new = run.corrected.unwrap_or_else(|| text.clone());
        if new == text {
            return Ok("[]".into());
        }
        Ok(format!(
            "[{{\"newText\":{},\"range\":{{\"start\":{{\"line\":0,\"character\":0}},\"end\":{{\"line\":{},\"character\":0}}}}}}]",
            json(&new),
            text.matches('\n').count() + 1
        ))
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

fn unsupported(id: &str, method: &str) {
    let message = format!("Unsupported Method: {method}");
    write(format!("{{\"id\":{id},\"error\":{{\"code\":-32601,\"message\":{}}}", json(&message)));
    log(&message);
}

/// Routes#convert_file_uri_to_path: `URI.decode_www_form_component(uri.delete_prefix('file://'))`.
fn uri_path(uri: &str) -> Result<String, String> {
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

/// Routes#change_text: apply one content change (a range, or the whole text).
fn change_text(orig: Option<&str>, text: String, range: Option<&Json>) -> Result<String, String> {
    let Some(range) = range.filter(|r| !matches!(r, Json::Null)) else {
        return Ok(text);
    };
    let orig = orig.ok_or("NoMethodError undefined method 'each_line' for nil")?;
    let start = text_pos(orig, range.get("start"))?;
    let end = text_pos(orig, range.get("end"))?.max(start);
    let b = orig.as_bytes();
    if start > b.len() {
        return Err(format!("IndexError index {start} out of string"));
    }
    let mut out = b[..start].to_vec();
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(&b[end.min(b.len())..]);
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Routes#text_pos: the byte offset of an LSP position (UTF-16 characters into its line).
fn text_pos(text: &str, pos: Option<&Json>) -> Result<usize, String> {
    let num = |k: &str| pos.and_then(|p| p.get(k)).and_then(Json::int).ok_or("NoMethodError position");
    let (line, character) = (num("line")?, num("character")?);
    let mut at = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i as i64 == line {
            // ponytail: a position inside a surrogate pair raises in RuboCop; here it rounds down.
            let mut units = 0;
            for c in l.chars() {
                units += c.len_utf16() as i64;
                if units > character {
                    break;
                }
                at += c.len_utf8();
            }
            return Ok(at);
        }
        at += l.len();
    }
    Ok(at)
}

/// A parsed JSON value (`JSON.parse(message, symbolize_names: true)`), keys in order.
#[derive(Clone, Debug, PartialEq)]
enum Json {
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
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn dig(&self, keys: &[&str]) -> Option<&Json> {
        keys.iter().try_fold(self, |j, k| j.get(k))
    }

    fn items(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => &[],
        }
    }

    fn int(&self) -> Option<i64> {
        match self {
            Json::Num(n) => n.parse().ok(),
            _ => None,
        }
    }

    /// A string's value; any other value as `to_s` would print it.
    fn string(&self) -> String {
        match self {
            Json::Str(s) => s.clone(),
            Json::Null => String::new(),
            other => other.text(),
        }
    }

    /// JSON.generate
    fn text(&self) -> String {
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

    fn parse(s: &str) -> Result<Json, String> {
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
    fn text_positions_count_utf16() {
        let t = "a😀b\nxy\n";
        let pos = |l: i64, c: i64| Json::parse(&format!("{{\"line\":{l},\"character\":{c}}}")).unwrap();
        assert_eq!(text_pos(t, Some(&pos(0, 3))).unwrap(), 5);
        assert_eq!(text_pos(t, Some(&pos(1, 1))).unwrap(), 8);
        assert_eq!(text_pos(t, Some(&pos(5, 0))).unwrap(), t.len());
        assert_eq!(utf16("a😀b", 2), 3);
    }

    #[test]
    fn uris() {
        assert_eq!(uri_path("file:///a%20b+c.rb").unwrap(), "/a b c.rb");
        assert_eq!(ensure_uri_scheme("/a/b.rb").unwrap(), "file:///a/b.rb");
        assert_eq!(ensure_uri_scheme("untitled:Untitled-1").unwrap(), "untitled:Untitled-1");
        assert!(ensure_uri_scheme("/a b.rb").is_err());
    }
}
