//! `--lsp`: RuboCop 1.91.0's language server (lib/rubocop/lsp/*), JSON-RPC over stdin/stdout.
//! Requests are handled one at a time, in order (RuboCop's server is single-threaded); each
//! response is written as RuboCop's `JSON.generate` writes it, `jsonrpc` last.

use crate::serve::{Json, Runtime, uri_path};
use rbcop::config::{Resolved, Store};
use rbcop::formatter::json;
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
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
    let mut server = Server { runtime: Runtime::new(store, pwd_config), safe_autocorrect: true, texts: HashMap::new() };
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
    runtime: Runtime,
    safe_autocorrect: bool,
    /// Routes' @text_cache: each open document's text, by URI.
    texts: HashMap<String, String>,
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
                let only = &mut self.runtime.only;
                only.clear();
                if matches!(option("lintMode"), Some(Json::Bool(true))) {
                    only.push("Lint".into());
                }
                if matches!(option("layoutMode"), Some(Json::Bool(true))) {
                    only.push("Layout".into());
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
        let diagnostics = self.runtime.offenses(&path, text)?;
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
        let new = self.runtime.format(&path, text.clone(), safe)?;
        if new == text {
            return Ok("[]".into());
        }
        Ok(format!(
            "[{{\"newText\":{},\"range\":{{\"start\":{{\"line\":0,\"character\":0}},\"end\":{{\"line\":{},\"character\":0}}}}}}]",
            json(&new),
            text.matches('\n').count() + 1
        ))
    }
}

fn unsupported(id: &str, method: &str) {
    let message = format!("Unsupported Method: {method}");
    write(format!("{{\"id\":{id},\"error\":{{\"code\":-32601,\"message\":{}}}", json(&message)));
    log(&message);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_positions_count_utf16() {
        let t = "a😀b\nxy\n";
        let pos = |l: i64, c: i64| Json::parse(&format!("{{\"line\":{l},\"character\":{c}}}")).unwrap();
        assert_eq!(text_pos(t, Some(&pos(0, 3))).unwrap(), 5);
        assert_eq!(text_pos(t, Some(&pos(1, 1))).unwrap(), 8);
        assert_eq!(text_pos(t, Some(&pos(5, 0))).unwrap(), t.len());
    }
}
