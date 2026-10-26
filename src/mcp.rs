//! `--mcp`: RuboCop 1.91.0's MCP server (lib/rubocop/mcp/server.rb) as the `mcp` gem 0.25.0
//! (the newest `~> 0.6`, which RuboCop's load error asks for) serves it: JSON-RPC over stdio, one
//! message per line, each answer one line written as `JSON.generate` writes it. Its two tools run
//! on the same LSP::Runtime as `--lsp`. RuboCop needs the gem installed; rbcop always serves.

use crate::serve::{Json, Runtime};
use rbcop::config::{RUBOCOP_VERSION, Resolved, Store, relative_path};
use rbcop::formatter::json;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

/// StdioTransport::MAX_LINE_BYTES: a longer line ends the server.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Configuration::SUPPORTED_STABLE_PROTOCOL_VERSIONS, the latest first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Server#default_capabilities
const CAPABILITIES: &str = "{\"tools\":{\"listChanged\":true},\"prompts\":{\"listChanged\":true},\"resources\":{\"listChanged\":true},\"logging\":{}}";

/// The tools RuboCop's MCP::Server#start defines, as Tool.to_h writes them.
const TOOLS: [&str; 2] = [
    "{\"name\":\"rubocop_inspection\",\"description\":\"Inspect Ruby code for offenses. Provide `source_code` to check \
     inline code or `path` to check files.\",\"inputSchema\":{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\
     \"properties\":{\"path\":{\"type\":\"string\"},\"source_code\":{\"type\":\"string\"}},\"type\":\"object\"},\
     \"annotations\":{\"destructiveHint\":false,\"idempotentHint\":true,\"openWorldHint\":false,\"readOnlyHint\":true,\
     \"title\":\"RuboCop's inspection\"}}",
    "{\"name\":\"rubocop_autocorrection\",\"description\":\"Autocorrect RuboCop offenses in Ruby code. Provide \
     `source_code` to correct inline code or `path` to correct files. Set `safety` to false to include unsafe \
     corrections.\",\"inputSchema\":{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\"properties\":\
     {\"path\":{\"type\":\"string\"},\"source_code\":{\"type\":\"string\"},\"safety\":{\"type\":\"boolean\"}},\
     \"required\":[\"safety\"],\"type\":\"object\"},\"annotations\":{\"destructiveHint\":true,\"idempotentHint\":false,\
     \"openWorldHint\":false,\"readOnlyHint\":false,\"title\":\"RuboCop's autocorrection\"}}",
];
const TOOL_NAMES: [&str; 2] = ["rubocop_inspection", "rubocop_autocorrection"];

/// A JSON-RPC error: code, message, data.
type Fail = (i32, &'static str, String);

fn internal(data: String) -> Fail {
    (-32603, "Internal error", data)
}

fn invalid_params(data: impl Into<String>) -> Fail {
    (-32602, "Invalid params", data.into())
}

/// Why a tool call failed: a RuboCop::Error (an error result) or anything else (an internal error).
enum ToolFail {
    Rubo(String),
    Other(String),
}

/// StdioTransport#open: answer each line until stdin ends (or a line is too long).
pub fn serve(store: Store, pwd_config: Arc<Resolved>) -> Result<u8, String> {
    let mut server = Server { runtime: Runtime::new(store, pwd_config), initialized: false };
    let mut input = std::io::stdin().lock();
    loop {
        let mut line = Vec::new();
        let n = input.by_ref().take(MAX_LINE_BYTES as u64).read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
        if n == 0 || (!line.ends_with(b"\n") && n >= MAX_LINE_BYTES) {
            return Ok(0);
        }
        // String#strip
        let text = String::from_utf8_lossy(&line);
        if let Some(answer) = server.handle_json(text.trim_matches(['\0', '\t', '\n', '\x0b', '\x0c', '\r', ' '])) {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{answer}");
            let _ = out.flush();
        }
    }
}

/// JsonRpcHandler#valid_id?: null, an integer, or a string of `[a-zA-Z0-9_-]`.
fn valid_id(id: &Json) -> bool {
    match id {
        Json::Null => true,
        Json::Num(n) => !n.contains(['.', 'e', 'E']),
        Json::Str(s) => !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        _ => false,
    }
}

/// JsonRpcHandler#error_response: `id` None is `:unknown_id` (answered with a null id); a request
/// without an id (a notification) gets no answer.
fn error(id: Option<&Json>, (code, message, data): (i32, &str, String)) -> Option<String> {
    let id = match id {
        None => "null".to_string(),
        Some(Json::Null) => return None,
        Some(id) if valid_id(id) => id.text(),
        Some(_) => "null".to_string(),
    };
    Some(format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{code},\"message\":{},\"data\":{}}}}}",
        json(message),
        json(&data)
    ))
}

fn invalid_request(id: Option<&Json>, data: &str) -> Option<String> {
    error(id, (-32600, "Invalid Request", data.to_string()))
}

/// Ruby's `inspect` of a keyword argument's name.
fn symbol(name: &str) -> String {
    let plain = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.trim_end_matches(['?', '!', '=']).chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain { format!(":{name}") } else { format!(":{}", json(name)) }
}

/// The class `NoMethodError` names for a non-Hash value (`for an instance of String`).
fn receiver(value: &Json) -> String {
    match value {
        Json::Bool(b) => b.to_string(),
        Json::Null => "nil".into(),
        Json::Num(n) if n.contains(['.', 'e', 'E']) => "an instance of Float".into(),
        Json::Num(_) => "an instance of Integer".into(),
        Json::Str(_) => "an instance of String".into(),
        Json::Arr(_) => "an instance of Array".into(),
        Json::Obj(_) => "an instance of Hash".into(),
    }
}

/// An I/O error as Ruby's Errno message (`No such file or directory @ rb_sysopen - <path>`).
fn errno(e: &std::io::Error, path: &str) -> String {
    let s = e.to_string();
    let s = s.split(" (os error").next().unwrap_or(&s);
    format!("{s} @ rb_sysopen - {path}")
}

struct Server {
    runtime: Runtime,
    /// ServerSession#initialized?
    initialized: bool,
}

impl Server {
    /// JsonRpcHandler.handle_json
    fn handle_json(&mut self, line: &str) -> Option<String> {
        let Ok(request) = Json::parse(line) else {
            return error(None, (-32700, "Parse error", "Invalid JSON".into()));
        };
        match &request {
            Json::Arr(list) if list.is_empty() => invalid_request(None, "Request is an empty array"),
            Json::Arr(list) => {
                let answers: Vec<String> = list.iter().filter_map(|r| self.process(r)).collect();
                match answers.len() {
                    0 => None,
                    1 => answers.into_iter().next(),
                    _ => Some(format!("[{}]", answers.join(","))),
                }
            }
            Json::Obj(_) => self.process(&request),
            _ => invalid_request(None, "Request must be an array or a hash"),
        }
    }

    /// JsonRpcHandler.process_request
    fn process(&mut self, request: &Json) -> Option<String> {
        if !matches!(request, Json::Obj(_)) {
            return invalid_request(None, "Request must be an object");
        }
        let id = request.get("id").unwrap_or(&Json::Null);
        let unknown = if matches!(id, Json::Null) { None } else { Some(id) };
        let method = match request.get("method") {
            Some(Json::Str(m)) if !m.starts_with("rpc.") => Some(m.as_str()),
            _ => None,
        };
        if request.get("jsonrpc") != Some(&Json::Str("2.0".into())) {
            return invalid_request(unknown, "JSON-RPC version must be 2.0");
        }
        if !valid_id(id) {
            return invalid_request(unknown, "Request ID must match validation pattern, or be an integer or null");
        }
        let Some(method) = method else {
            return invalid_request(unknown, "Method name must be a string and not start with \"rpc.\"");
        };
        let params = request.get("params").filter(|p| !matches!(p, Json::Null));
        if params.is_some_and(|p| !matches!(p, Json::Arr(_) | Json::Obj(_))) {
            return error(Some(id), invalid_params("Method parameters must be an array or an object or null"));
        }
        let not_found = || error(Some(id), (-32601, "Method not found", method.to_string()));
        // Server#handle_request: a notification sent as a request is not found.
        let notification = method.starts_with("notifications/");
        if notification && !matches!(id, Json::Null) {
            return not_found();
        }
        // Methods.ensure_capability!: these need capabilities RuboCop's server does not declare.
        let missing = match method {
            "completion/complete" => Some("completions"),
            "resources/subscribe" | "resources/unsubscribe" => Some("resources.subscribe"),
            _ => None,
        };
        if let Some(capability) = missing {
            return error(Some(id), internal(format!("Server does not support {capability} (required for {method})")));
        }
        let result = match method {
            "initialize" => self.initialize(params),
            "ping" => Ok("{}".into()),
            "server/discover" => Ok(format!(
                "{{\"supportedVersions\":[{}],\"capabilities\":{CAPABILITIES},\"serverInfo\":{}}}",
                PROTOCOL_VERSIONS.map(json).join(","),
                server_info()
            )),
            "notifications/initialized"
            | "notifications/progress"
            | "notifications/roots/list_changed"
            | "notifications/cancelled" => return None,
            "tools/list" => list(params, &TOOLS).map(|t| format!("{{\"tools\":[{t}]}}")),
            "prompts/list" => list(params, &[]).map(|_| "{\"prompts\":[]}".into()),
            "resources/list" => list(params, &[]).map(|_| "{\"resources\":[]}".into()),
            "resources/templates/list" => list(params, &[]).map(|_| "{\"resourceTemplates\":[]}".into()),
            // No resources or prompts: any uri reads nothing, any prompt is missing.
            "resources/read" => match params {
                Some(Json::Obj(_)) => Ok("{\"contents\":[]}".into()),
                _ => Err(internal("Internal error handling resources/read request".into())),
            },
            "prompts/get" => match params {
                Some(p @ Json::Obj(_)) => {
                    Err(internal(format!("Prompt not found {}", p.get("name").map_or(String::new(), Json::string))))
                }
                _ => Err(internal("Internal error handling prompts/get request".into())),
            },
            "logging/setLevel" => match params {
                Some(p @ Json::Obj(_)) => {
                    let level = p.get("level");
                    let levels = ["debug", "info", "notice", "warning", "error", "critical", "alert", "emergency"];
                    if level.is_some_and(|l| matches!(l, Json::Str(s) if levels.contains(&s.as_str()))) {
                        Ok("{}".into())
                    } else {
                        Err(invalid_params(format!("Invalid log level {}", level.map_or(String::new(), Json::string))))
                    }
                }
                _ => Err(internal("Internal error handling logging/setLevel request".into())),
            },
            "tools/call" => self.call_tool(params),
            _ => return not_found(),
        };
        match result {
            Ok(_) if matches!(id, Json::Null) => None,
            Ok(result) => Some(format!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{result}}}", id.text())),
            Err(fail) => error(Some(id), fail),
        }
    }

    /// Server#init
    fn initialize(&mut self, params: Option<&Json>) -> Result<String, Fail> {
        let Some(p @ Json::Obj(_)) = params else { return Err(invalid_params("Invalid params")) };
        let Some(Json::Str(version)) = p.get("protocolVersion") else {
            return Err(invalid_params("Missing or invalid protocolVersion"));
        };
        if !matches!(p.get("capabilities"), Some(Json::Obj(_))) {
            return Err(invalid_params("Missing or invalid capabilities"));
        }
        let present = |key: &str| p.dig(&["clientInfo", key]).is_some_and(|v| !matches!(v, Json::Null));
        if !matches!(p.get("clientInfo"), Some(Json::Obj(_))) || !present("name") || !present("version") {
            return Err(invalid_params("Missing or invalid clientInfo"));
        }
        if self.initialized {
            return Err((-32600, "Invalid Request", "Invalid Request: Server already initialized".into()));
        }
        self.initialized = true;
        let negotiated = PROTOCOL_VERSIONS.iter().find(|v| *v == version).unwrap_or(&PROTOCOL_VERSIONS[0]);
        Ok(format!(
            "{{\"protocolVersion\":{},\"capabilities\":{CAPABILITIES},\"serverInfo\":{}}}",
            json(negotiated),
            server_info()
        ))
    }

    /// Server#call_tool
    fn call_tool(&mut self, params: Option<&Json>) -> Result<String, Fail> {
        let request = match params {
            Some(p @ Json::Obj(_)) => p,
            Some(_) => {
                return Err(internal(
                    "Internal error calling tool : no implicit conversion of Symbol into Integer".into(),
                ));
            }
            None => return Err(internal("Internal error calling tool : undefined method '[]' for nil".into())),
        };
        let name = request.get("name");
        let Some(&tool) = TOOL_NAMES.iter().find(|t| name == Some(&Json::Str(t.to_string()))) else {
            return Err(invalid_params(format!("Tool not found: {}", name.map_or(String::new(), Json::string))));
        };
        let empty = Json::Obj(Vec::new());
        let arguments = match request.get("arguments") {
            None | Some(Json::Null | Json::Bool(false)) => &empty,
            Some(a) => a,
        };
        let autocorrection = tool == "rubocop_autocorrection";
        let tool_error = |text: String| {
            Ok(format!("{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":true}}", json(&text)))
        };
        // InputSchema#missing_required_arguments, then #validate_arguments (json_schemer).
        if autocorrection {
            let Json::Obj(args) = arguments else {
                return Err(internal(format!(
                    "Internal error calling tool {tool}: undefined method 'keys' for {}",
                    receiver(arguments)
                )));
            };
            if !args.iter().any(|(k, _)| k == "safety") {
                return tool_error("Missing required arguments: safety".into());
            }
        }
        let Json::Obj(args) = arguments else {
            return tool_error("Invalid arguments: value at root is not an object".into());
        };
        let schema: &[(&str, &str)] = if autocorrection {
            &[("path", "string"), ("source_code", "string"), ("safety", "boolean")]
        } else {
            &[("path", "string"), ("source_code", "string")]
        };
        let invalid: Vec<String> = schema
            .iter()
            .filter(|(key, kind)| {
                arguments.get(key).is_some_and(|v| match *kind {
                    "string" => !matches!(v, Json::Str(_)),
                    _ => !matches!(v, Json::Bool(_)),
                })
            })
            .map(|(key, kind)| format!("value at `/{key}` is not a {kind}"))
            .collect();
        if !invalid.is_empty() {
            return tool_error(format!("Invalid arguments: {}", invalid.join(", ")));
        }
        // The tool's block takes `path:`, `source_code:` and `safety:`; others are an ArgumentError.
        let unknown: Vec<String> = args
            .iter()
            .map(|(k, _)| k.as_str())
            .filter(|k| !["path", "source_code", "safety"].contains(k))
            .map(symbol)
            .collect();
        if !unknown.is_empty() {
            let s = if unknown.len() == 1 { "" } else { "s" };
            return Err(internal(format!(
                "Internal error calling tool {tool}: unknown keyword{s}: {}",
                unknown.join(", ")
            )));
        }
        let string = |key: &str| match arguments.get(key) {
            Some(Json::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        let (path, source) = (string("path"), string("source_code"));
        let result = if autocorrection {
            let safety = !matches!(arguments.get("safety"), Some(Json::Bool(false)));
            self.run_autocorrection(path, source, safety)
        } else {
            self.run_inspection(path, source)
        };
        match result {
            Ok(text) => {
                Ok(format!("{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":false}}", json(&text)))
            }
            Err(ToolFail::Rubo(text)) => tool_error(text),
            Err(ToolFail::Other(e)) => Err(internal(format!("Internal error calling tool {tool}: {e}"))),
        }
    }

    fn run_inspection(&mut self, path: Option<&str>, source: Option<&str>) -> Result<String, ToolFail> {
        if let Some(source) = source {
            let offenses =
                self.runtime.offenses(path.unwrap_or("example.rb"), source.to_string()).map_err(ToolFail::Other)?;
            return Ok(format!("[{}]", offenses.join(",")));
        }
        let targets = self.targets(path)?;
        let mut files = Vec::new();
        let mut count = 0;
        for file in &targets {
            let abs = file.display().to_string();
            let source = read_file(&abs)?;
            let offenses = self.runtime.offenses(&abs, source).map_err(ToolFail::Other)?;
            count += offenses.len();
            if !offenses.is_empty() {
                files.push(format!(
                    "{{\"path\":{},\"offenses\":[{}]}}",
                    json(&relative_path(file, &self.runtime.cwd)),
                    offenses.join(",")
                ));
            }
        }
        // Server#build_summary judges by the first file: with none, an inspection counts corrections.
        let counted = if targets.is_empty() { "corrected_file_count" } else { "offense_count" };
        Ok(summary(&files, targets.len(), counted, count))
    }

    fn run_autocorrection(&mut self, path: Option<&str>, source: Option<&str>, safe: bool) -> Result<String, ToolFail> {
        if let Some(source) = source {
            let corrected =
                self.runtime.format(path.unwrap_or("example.rb"), source.to_string(), safe).map_err(ToolFail::Other)?;
            if let Some(path) = path {
                self.write_file(path, &corrected)?;
            }
            return Ok(corrected);
        }
        let targets = self.targets(path)?;
        let mut files = Vec::new();
        let mut count = 0;
        for file in &targets {
            let abs = file.display().to_string();
            let source = read_file(&abs)?;
            let corrected = self.runtime.format(&abs, source.clone(), safe).map_err(ToolFail::Other)?;
            self.write_file(&abs, &corrected)?;
            let changed = source != corrected;
            count += usize::from(changed);
            files.push(format!(
                "{{\"path\":{},\"corrected\":{changed}}}",
                json(&relative_path(file, &self.runtime.cwd))
            ));
        }
        Ok(summary(&files, targets.len(), "corrected_file_count", count))
    }

    /// TargetFinder#find(path ? [path] : [], :only_recognized_file_types)
    fn targets(&mut self, path: Option<&str>) -> Result<Vec<PathBuf>, ToolFail> {
        let args: Vec<PathBuf> = path.map(PathBuf::from).into_iter().collect();
        rbcop::targets::find(&mut self.runtime.store, &args, false, true).map_err(ToolFail::Other)
    }

    /// Server#write_file: `File.write`, its likely errors as RuboCop::Error.
    fn write_file(&self, path: &str, content: &str) -> Result<(), ToolFail> {
        std::fs::write(self.runtime.cwd.join(path), content).map_err(|e| match e.raw_os_error() {
            Some(13) => ToolFail::Rubo(format!("Permission denied: {path}")),
            Some(28) => ToolFail::Rubo(format!("No space left on device: {path}")),
            Some(30) => ToolFail::Rubo(format!("Read-only file system: {path}")),
            _ => ToolFail::Other(errno(&e, path)),
        })
    }
}

/// Server#read_file: the file's text; a missing file is a RuboCop::Error.
fn read_file(abs: &str) -> Result<String, ToolFail> {
    // ponytail: invalid UTF-8 is read lossily; RuboCop keeps the bytes and reports Lint/Syntax.
    std::fs::read(abs).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ToolFail::Rubo(format!("No such file or directory: {abs}")),
        _ => ToolFail::Other(errno(&e, abs)),
    })
}

/// Server#process_files' result.
fn summary(files: &[String], targets: usize, counted: &str, count: usize) -> String {
    format!("{{\"files\":[{}],\"summary\":{{\"target_file_count\":{targets},\"{counted}\":{count}}}}}", files.join(","))
}

/// Server#server_info
fn server_info() -> String {
    format!("{{\"name\":\"rubocop_mcp_server\",\"version\":{}}}", json(RUBOCOP_VERSION))
}

/// Pagination#cursor_from and #paginate (no page size): the items from the cursor on, joined.
fn list(params: Option<&Json>, items: &[&str]) -> Result<String, Fail> {
    let cursor = match params {
        None => None,
        Some(p @ Json::Obj(_)) => p.get("cursor").filter(|c| !matches!(c, Json::Null)),
        Some(_) => return Err(invalid_params("Invalid params")),
    };
    let start = match cursor {
        None => 0,
        // ponytail: Kernel#Integer also takes `0x1`, `1_0` and surrounding spaces.
        Some(Json::Str(s)) => match s.parse::<usize>() {
            Ok(n) if n < items.len() => n,
            _ => return Err(invalid_params("Invalid cursor")),
        },
        Some(_) => return Err(invalid_params("Invalid cursor")),
    };
    Ok(items[start..].join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_symbols() {
        assert!(valid_id(&Json::Num("3".into())) && !valid_id(&Json::Num("1.5".into())));
        assert!(valid_id(&Json::Str("ab-1".into())) && !valid_id(&Json::Str("a b".into())));
        assert_eq!(symbol("foo"), ":foo");
        assert_eq!(symbol("a-b"), ":\"a-b\"");
        assert_eq!(list(None, &TOOLS).unwrap().matches("\"name\"").count(), 2);
        assert!(list(Some(&Json::Obj(vec![("cursor".into(), Json::Str("2".into()))])), &TOOLS).is_err());
    }
}
