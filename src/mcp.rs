//! `--mcp`: RuboCop 1.92.1's MCP server (lib/rubocop/mcp/*.rb) as the `mcp` gem 0.25.0 serves
//! it: JSON-RPC over stdio, one message per line, each answer one line written as `JSON.generate`
//! writes it. The inspection and autocorrection tools run on the same LSP::Runtime as `--lsp`,
//! without LSP mode (offenses as `--format json` reports them) and with cop errors collected per
//! file; `rubocop_explain` is `--explain` (src/explain.rs). RuboCop needs the gem installed;
//! rbcop always serves.

use crate::serve::{CallOptions, Json, Runtime};
use rbcop::autocorrect;
use rbcop::config::validator::smart_path;
use rbcop::config::{RUBOCOP_VERSION, Resolved, Store, expand_path};
use rbcop::formatter::{Off, json};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// StdioTransport::MAX_LINE_BYTES: a longer line ends the server.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Configuration::SUPPORTED_STABLE_PROTOCOL_VERSIONS, the latest first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Server#default_capabilities
const CAPABILITIES: &str = "{\"tools\":{\"listChanged\":true},\"prompts\":{\"listChanged\":true},\"resources\":{\"listChanged\":true},\"logging\":{}}";

/// The tools RuboCop 1.92.1's MCP::Server#start defines, as Tool.to_h writes them.
const TOOLS: [&str; 3] = [
    r#"{"name":"rubocop_inspection","description":"Inspect Ruby code for offenses. Provide `source_code` to check inline code or `path` to check files. Either way the result lists offenses per file, with a summary, and the offenses use the `rubocop --format json` format, with 1-based lines. `correctable` says whether `rubocop_autocorrection` fixes an offense, and one whose `correction` is not `safe` needs `safety` set to false. One with a `correction` that is not `correctable` is held back while code is being written, and only fixed with `contextual` set to true. `max_offenses_per_cop` caps what each cop reports, and the summary's `unreported_offenses` counts what was left out. A cop that crashes is listed in its file's `errors`; the rest still report.","inputSchema":{"$schema":"https://json-schema.org/draft/2020-12/schema","properties":{"path":{"type":"string"},"source_code":{"type":"string"},"max_offenses_per_cop":{"type":"integer","minimum":1},"only":{"type":"array","items":{"type":"string"},"description":"Run only these cops or departments, such as `Style/StringLiterals` or `Lint`."},"except":{"type":"array","items":{"type":"string"},"description":"Run every cop except these cops or departments."},"changed":{"type":["boolean","string"],"description":"Check only the files git says changed: `true` for changes since the last commit, or a revision to compare against, such as `main`."}},"additionalProperties":false,"type":"object"},"annotations":{"destructiveHint":false,"idempotentHint":true,"openWorldHint":false,"readOnlyHint":true,"title":"RuboCop's inspection"}}"#,
    r#"{"name":"rubocop_autocorrection","description":"Autocorrect RuboCop offenses in Ruby code. Provide `source_code` to correct inline code, which comes back corrected as plain text, or `path` to correct files. Set `safety` to false to include unsafe corrections, and `contextual` to true to include the ones held back while code is being written, such as removing an unused variable. Set `dry_run` to true to preview: nothing is written, and each corrected file comes with its `diff`. For files, the result lists each file that was corrected or still has offenses, with the offenses left in the `rubocop_inspection` format, located in the corrected file. One whose `correction` is not `safe` needs `safety` set to false, and one with a `correction` that is not `correctable` needs `contextual` set to true. Each cop lists at most `max_offenses_per_cop` of them, 5 unless set, and the summary's `unreported_offenses` counts the rest. A cop that crashes is listed in its file's `errors`, and a file whose corrections loop is left unchanged.","inputSchema":{"$schema":"https://json-schema.org/draft/2020-12/schema","properties":{"path":{"type":"string"},"source_code":{"type":"string"},"safety":{"type":"boolean"},"contextual":{"type":"boolean"},"dry_run":{"type":"boolean"},"max_offenses_per_cop":{"type":"integer","minimum":1},"only":{"type":"array","items":{"type":"string"},"description":"Run only these cops or departments, such as `Style/StringLiterals` or `Lint`."},"except":{"type":"array","items":{"type":"string"},"description":"Run every cop except these cops or departments."},"changed":{"type":["boolean","string"],"description":"Check only the files git says changed: `true` for changes since the last commit, or a revision to compare against, such as `main`."}},"required":["safety"],"additionalProperties":false,"type":"object"},"annotations":{"destructiveHint":true,"idempotentHint":false,"openWorldHint":false,"readOnlyHint":false,"title":"RuboCop's autocorrection"}}"#,
    r#"{"name":"rubocop_explain","description":"Explain what cops do: the problem each one targets, bad and good examples, its configuration as this project resolves it, and whether it autocorrects. Takes cop names as offenses report them, such as `Style/StringLiterals`. Pass the `path` of a file to see the configuration that applies to it.","inputSchema":{"$schema":"https://json-schema.org/draft/2020-12/schema","properties":{"cop_names":{"type":"array","items":{"type":"string"},"minItems":1},"path":{"type":"string"}},"required":["cop_names"],"additionalProperties":false,"type":"object"},"annotations":{"destructiveHint":false,"idempotentHint":true,"openWorldHint":false,"readOnlyHint":true,"title":"RuboCop's cop explanation"}}"#,
];
const TOOL_NAMES: [&str; 3] = ["rubocop_inspection", "rubocop_autocorrection", "rubocop_explain"];

/// Version control metadata directories the tools never read or write under (in any case).
const VCS_DIRS: [&str; 4] = [".git", ".hg", ".svn", ".jj"];

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

impl From<crate::serve::Fail> for ToolFail {
    fn from(f: crate::serve::Fail) -> ToolFail {
        match f {
            crate::serve::Fail::Rubo(e) => ToolFail::Rubo(e),
            crate::serve::Fail::Other(e) => ToolFail::Other(e),
        }
    }
}

/// StdioTransport#open: answer each line until stdin ends (or a line is too long).
pub fn serve(store: Store, pwd_config: Arc<Resolved>) -> Result<u8, String> {
    let roots = autocorrect::write_roots(store.cwd(), &[]);
    // An agent isn't an editor: offenses are located and worded as on the command line, and one
    // cop crashing on one file does not cost the rest of the request.
    // Offenses carry their corrections, as `--format json` reports them.
    rbcop::cop::record_edits();
    let mut runtime = Runtime::new(store, pwd_config, false);
    runtime.raise_cop_error = false;
    let mut server = Server { runtime, roots, initialized: false };
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
    /// The working directory, canonical: the only root the tools read and write under.
    roots: Vec<PathBuf>,
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

    /// Server#call_tool, then Tool.call: the arguments checked against the tool's input schema
    /// (InputSchema#missing_required_arguments, then #validate_arguments with json_schemer), and
    /// the tool's block (build_tool's, with its Scope, or ExplainTool's).
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
        let tool_error = |text: String| {
            Ok(format!("{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":true}}", json(&text)))
        };
        let schema = schema(tool);
        let required = match tool {
            "rubocop_autocorrection" => Some("safety"),
            "rubocop_explain" => Some("cop_names"),
            _ => None,
        };
        if let Some(required) = required {
            let Json::Obj(args) = arguments else {
                return Err(internal(format!(
                    "Internal error calling tool {tool}: undefined method 'keys' for {}",
                    receiver(arguments)
                )));
            };
            if !args.iter().any(|(k, _)| k == required) {
                return tool_error(format!("Missing required arguments: {required}"));
            }
        }
        let Json::Obj(args) = arguments else {
            return tool_error("Invalid arguments: value at root is not an object".into());
        };
        let mut invalid: Vec<String> = schema
            .iter()
            .filter_map(|(key, kind)| arguments.get(key).map(|v| kind.errors(&format!("/{key}"), v)))
            .flatten()
            .collect();
        // additionalProperties: false
        invalid.extend(
            args.iter()
                .filter(|(k, _)| !schema.iter().any(|(key, _)| key == k))
                .map(|(k, _)| format!("object property at `/{k}` is a disallowed additional property")),
        );
        if !invalid.is_empty() {
            return tool_error(format!("Invalid arguments: {}", invalid.join(", ")));
        }
        let result = if tool == "rubocop_explain" { self.explain(arguments) } else { self.run_tool(tool, arguments) };
        match result {
            Ok(text) => {
                Ok(format!("{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":false}}", json(&text)))
            }
            Err(ToolFail::Rubo(text)) => tool_error(text),
            Err(ToolFail::Other(e)) => Err(internal(format!("Internal error calling tool {tool}: {e}"))),
        }
    }

    /// build_tool's block: the Scope from `only`, `except` and `changed`, then the tool's own.
    fn run_tool(&mut self, tool: &str, arguments: &Json) -> Result<String, ToolFail> {
        let string = |key: &str| match arguments.get(key) {
            Some(Json::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        let flag = |key: &str| match arguments.get(key) {
            Some(Json::Bool(b)) => Some(*b),
            _ => None,
        };
        let (path, source) = (string("path"), string("source_code"));
        let scope = Scope::new(source.is_some(), arguments)?;
        let max = arguments.get("max_offenses_per_cop").and_then(Limit::max);
        if tool == "rubocop_inspection" {
            return self.run_inspection(path, source, max.map(Limit::new), &scope);
        }
        let request = Request {
            safety: flag("safety").unwrap_or(true),
            contextual: flag("contextual").unwrap_or(false),
            dry_run: flag("dry_run").unwrap_or(false),
        };
        let mut limit = Limit::new(max.unwrap_or(DEFAULT_MAX_OFFENSES_PER_COP));
        self.run_autocorrection(path, source, &request, &mut limit, &scope)
    }

    /// Server#run_inspection: a limit counts across the files of one request.
    fn run_inspection(
        &mut self,
        path: Option<&str>,
        source: Option<&str>,
        mut limit: Option<Limit>,
        scope: &Scope,
    ) -> Result<String, ToolFail> {
        let mut result = if let Some(source) = source {
            let file = path.unwrap_or("example.rb");
            // Only a name here, but its config is looked up next to it: keep that inside too.
            self.contain(file)?;
            let entry = self.inspect_source(file, source.to_string(), limit.as_mut(), scope)?;
            build_result(1, vec![entry])
        } else {
            let targets = self.targets(path, scope)?;
            let mut entries = Vec::new();
            for file in &targets {
                let abs = file.display().to_string();
                let source = self.read_file(&abs)?;
                entries.push(self.inspect_source(&abs, source, limit.as_mut(), scope)?);
            }
            build_result(targets.len(), entries)
        };
        Ok(result_json(&mut result, limit.as_ref()))
    }

    /// Server#inspect_source: the offenses as they are, contextual corrections held back.
    fn inspect_source(
        &mut self,
        file: &str,
        source: String,
        limit: Option<&mut Limit>,
        scope: &Scope,
    ) -> Result<Entry, ToolFail> {
        let options = CallOptions { editing: true, ..scope.cop_options() };
        let run = self.runtime.raw_offenses(file, source, &options)?;
        Ok(self.offense_entry(file, run.offs(|_| true), limit, &run.cfg))
    }

    /// Server#offense_entry and #problems_of_last_run: the file's `--format json` entry, its
    /// offenses capped across the request.
    fn offense_entry(&self, file: &str, offenses: Vec<Off>, limit: Option<&mut Limit>, cfg: &Resolved) -> Entry {
        let offenses = match limit {
            Some(limit) => offenses.into_iter().filter(|o| limit.admit(o.cop)).collect(),
            None => offenses,
        };
        let cwd = &self.runtime.cwd;
        Entry {
            path: smart_path(Path::new(file), cwd),
            offenses: offenses.iter().map(|o| rbcop::formatter::hash_for_offense(o, cfg)).collect(),
            problems: rbcop::formatter::hash_for_problems(
                &smart_path(Path::new(file), cwd),
                &expand_path(Path::new(file), cwd),
                &self.runtime.errors,
                &self.runtime.warnings,
            ),
            corrected: None,
            diff: None,
        }
    }

    /// Server#run_autocorrection
    fn run_autocorrection(
        &mut self,
        path: Option<&str>,
        source: Option<&str>,
        request: &Request,
        limit: &mut Limit,
        scope: &Scope,
    ) -> Result<String, ToolFail> {
        if let Some(source) = source {
            return self.correct_inline(path, source, request, scope);
        }
        let targets = self.targets(path, scope)?;
        let mut entries = Vec::new();
        for file in &targets {
            let abs = file.display().to_string();
            let source = self.read_file(&abs)?;
            entries.push(self.correct_file(&abs, source, request, limit, scope)?);
        }
        let mut result = build_result(targets.len(), entries);
        let corrected = result.files.iter().filter(|f| f.corrected == Some(true)).count();
        result.summary.push(("corrected_file_count", corrected.to_string()));
        Ok(result_json(&mut result, Some(limit)))
    }

    /// Server#correct_file: a file whose corrections loop is left alone, and so is one with
    /// nothing to correct. The offenses left are located in the file as written.
    fn correct_file(
        &mut self,
        file: &str,
        source: String,
        request: &Request,
        limit: &mut Limit,
        scope: &Scope,
    ) -> Result<Entry, ToolFail> {
        let options = request.runtime_options(scope);
        let run = self.runtime.format_run(file, source.clone(), request.safety, &options)?;
        let corrected = run.corrected.clone().unwrap_or_default();
        let problems = (self.runtime.errors.clone(), self.runtime.warnings.clone());
        let changed = !run.looped && source != corrected;
        // Server#leftover_offenses (Util.emulate_write_read_cycle changes nothing off Windows).
        let (offenses, cfg) = if changed || source == corrected {
            (run.offs(|o| !o.corrected), run.cfg.clone())
        } else {
            let again = self.runtime.raw_offenses(file, source.clone(), &options)?;
            (again.offs(|_| true), again.cfg.clone())
        };
        (self.runtime.errors, self.runtime.warnings) = problems;
        let mut entry = self.offense_entry(file, offenses, Some(limit), &cfg);
        entry.corrected = Some(changed);
        if changed {
            if request.dry_run {
                entry.diff = Some(rbcop::unified_diff::unified_diff(&entry.path, &source, &corrected));
            } else {
                self.write_file(file, &corrected)?;
            }
        }
        Ok(entry)
    }

    /// Server#correct_inline: corrected code comes back as plain text with nowhere to mention a
    /// cop that crashed, so a crash fails the call.
    fn correct_inline(
        &mut self,
        path: Option<&str>,
        source: &str,
        request: &Request,
        scope: &Scope,
    ) -> Result<String, ToolFail> {
        let file = path.unwrap_or("example.rb");
        self.contain(file)?;
        let options = request.runtime_options(scope);
        let run = self.runtime.format_run(file, source.to_string(), request.safety, &options)?;
        let abs = expand_path(Path::new(file), &self.runtime.cwd);
        let errors = problem_messages(&abs, &self.runtime.cwd, &self.runtime.errors);
        if !errors.is_empty() {
            return Err(ToolFail::Rubo(errors.join("\n")));
        }
        let corrected = run.corrected.unwrap_or_default();
        if let Some(path) = path
            && !request.dry_run
        {
            self.write_file(path, &corrected)?;
        }
        Ok(corrected)
    }

    /// ExplainTool#explain: the cops it knows, then the names it does not, as `--explain` does,
    /// with the configuration that applies to `path` (or the working directory's).
    fn explain(&mut self, arguments: &Json) -> Result<String, ToolFail> {
        let names: Vec<String> =
            arguments.get("cop_names").map_or(&[][..], Json::items).iter().map(Json::string).collect();
        let cfg = match arguments.get("path") {
            Some(Json::Str(path)) => {
                let real = self.contain(path)?;
                self.runtime.store.for_file(&real)
            }
            _ => {
                let cwd = self.runtime.cwd.clone();
                self.runtime.store.for_dir(&cwd)
            }
        }
        .map_err(ToolFail::Rubo)?;
        let (text, unknown) = crate::explain::explain(&names, &cfg, &self.runtime.cwd);
        // `--explain` puts each explanation, a blank line between; the tool joins them the same way.
        let text = text.strip_suffix('\n').unwrap_or(&text);
        match unknown {
            Ok(()) => Ok(text.to_string()),
            Err(e) if text.is_empty() => Err(ToolFail::Rubo(e)),
            Err(e) => Err(ToolFail::Rubo(format!("{text}\n\n{e}"))),
        }
    }

    /// Where `path` really is (symlinks resolved, a missing tail kept as named), or a tool error
    /// when that is outside the working directory or inside version control metadata
    /// (VCS_DIRS). RuboCop's tools read and write wherever the client points; rbcop's stay inside
    /// the project, the root `-a` writes under.
    fn contain(&self, path: &str) -> Result<PathBuf, ToolFail> {
        let mut base = self.runtime.cwd.join(path);
        let mut rest = Vec::new();
        let real = loop {
            if let Ok(real) = std::fs::canonicalize(&base) {
                break Some(real);
            }
            // A `..` (or root) that does not resolve is refused rather than guessed at.
            match base.file_name() {
                Some(name) if base.ends_with(name) => rest.push(name.to_os_string()),
                _ => break None,
            }
            base.pop();
        };
        let real = real.map(|r| rest.iter().rev().fold(r, |p, c| p.join(c)));
        // A tool that wrote `.git/config` could make the next `changed` scope's git run a command
        // (core.fsmonitor and the like), so nothing under a VCS directory is read or written.
        let metadata = real.as_ref().filter(|r| {
            self.roots.iter().filter_map(|root| r.strip_prefix(root).ok()).any(|below| {
                below
                    .components()
                    .any(|c| VCS_DIRS.contains(&c.as_os_str().to_string_lossy().to_ascii_lowercase().as_str()))
            })
        });
        if let Some(r) = metadata {
            return Err(ToolFail::Rubo(format!("{path} resolves to {}, inside version control metadata", r.display())));
        }
        match real {
            Some(real) if !autocorrect::outside_roots(&real, &self.roots) => Ok(real),
            real => Err(ToolFail::Rubo(format!(
                "{path} resolves to {}, outside the project",
                real.map_or_else(|| path.to_string(), |r| r.display().to_string())
            ))),
        }
    }

    /// TargetFinder#find(path ? [path] : [], :only_recognized_file_types), then Scope#select_files.
    fn targets(&mut self, path: Option<&str>, scope: &Scope) -> Result<Vec<PathBuf>, ToolFail> {
        if let Some(path) = path {
            self.contain(path)?;
        }
        let args: Vec<PathBuf> = path.map(PathBuf::from).into_iter().collect();
        let files = rbcop::targets::find(&mut self.runtime.store, &args, false, true).map_err(ToolFail::Other)?;
        let Some(revision) = &scope.changed else { return Ok(files) };
        let changed = rbcop::targets::changed(revision).map_err(ToolFail::Rubo)?;
        Ok(files.into_iter().filter(|f| changed.contains(f)).collect())
    }

    /// SourceFiles.read: the file's text; a missing file is a RuboCop::Error.
    fn read_file(&self, abs: &str) -> Result<String, ToolFail> {
        let real = self.contain(abs)?;
        // ponytail: invalid UTF-8 is read lossily; RuboCop keeps the bytes and reports Lint/Syntax.
        std::fs::read(real).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ToolFail::Rubo(format!("No such file or directory: {abs}")),
            _ => ToolFail::Other(errno(&e, abs)),
        })
    }

    /// SourceFiles.write: nothing when the file already holds `content` (its modification time
    /// stays), else Util.replace_file_contents, which creates a missing file; its likely errors
    /// are RuboCop::Errors. Through autocorrect::write_source as `-a` writes, never via a symlink.
    fn write_file(&self, path: &str, content: &str) -> Result<(), ToolFail> {
        let real = self.contain(path)?;
        if real.is_file() && std::fs::read(&real).is_ok_and(|b| b == content.as_bytes()) {
            return Ok(());
        }
        let written = (|| {
            if !real.exists() {
                use std::os::unix::fs::OpenOptionsExt;
                // O_EXCL: never an existing file, nor a symlink planted since.
                std::fs::OpenOptions::new().write(true).create_new(true).custom_flags(libc::O_NOFOLLOW).open(&real)?;
            }
            autocorrect::write_source(&real, content, &self.roots)
        })();
        written.map_err(|e| match e.raw_os_error() {
            Some(libc::EPERM) => ToolFail::Rubo(format!("Operation not permitted: {path}")),
            Some(libc::EACCES) => ToolFail::Rubo(format!("Permission denied: {path}")),
            Some(libc::ENOSPC) => ToolFail::Rubo(format!("No space left on device: {path}")),
            Some(libc::EROFS) => ToolFail::Rubo(format!("Read-only file system: {path}")),
            // write_source's refusal (a path that moved outside, a symlink below the root).
            _ if e.kind() == std::io::ErrorKind::PermissionDenied => ToolFail::Rubo(e.to_string()),
            _ => ToolFail::Other(errno(&e, path)),
        })
    }
}

/// AutocorrectionRequest::DEFAULT_MAX_OFFENSES_PER_COP
const DEFAULT_MAX_OFFENSES_PER_COP: (f64, bool) = (5.0, false);

/// AutocorrectionRequest: what an autocorrection asks for, shared by every file it corrects.
struct Request {
    safety: bool,
    contextual: bool,
    dry_run: bool,
}

impl Request {
    /// AutocorrectionRequest#runtime_options: contextual corrections only on request.
    fn runtime_options(&self, scope: &Scope) -> CallOptions {
        CallOptions { editing: !self.contextual, ..scope.cop_options() }
    }
}

/// MCP::Scope: the cops `only` and `except` select, and the git revision `changed` compares to.
struct Scope {
    only: Vec<String>,
    except: Vec<String>,
    changed: Option<String>,
}

impl Scope {
    /// Scope.new: an empty list counts as no list, `changed: true` is HEAD, and what the
    /// command line refuses for the same options is refused. Cop names are left to the runner.
    fn new(inline: bool, arguments: &Json) -> Result<Scope, ToolFail> {
        let list = |key: &str| arguments.get(key).map_or(Vec::new(), |v| v.items().iter().map(Json::string).collect());
        let (only, except): (Vec<String>, Vec<String>) = (list("only"), list("except"));
        let changed = match arguments.get("changed") {
            Some(Json::Bool(true)) => Some("HEAD".to_string()),
            Some(Json::Str(s)) => Some(s.clone()),
            _ => None,
        };
        let refuse = |m: &str| Err(ToolFail::Rubo(m.to_string()));
        if inline && changed.is_some() {
            return refuse("`changed` only applies when checking files.");
        }
        if changed.as_deref() == Some("") {
            return refuse("`changed` takes `true` or a git revision.");
        }
        // OptionsValidator#only_includes_redundant_disable? and #except_syntax?
        if only.iter().any(|c| c == "Lint/RedundantCopDisableDirective" || c == "RedundantCopDisableDirective") {
            return refuse("Lint/RedundantCopDisableDirective cannot be used with `only`.");
        }
        if except.iter().any(|c| c == "Lint/Syntax" || c == "Syntax") {
            return refuse("Syntax checking cannot be turned off.");
        }
        Ok(Scope { only, except, changed })
    }

    /// Scope#cop_options
    fn cop_options(&self) -> CallOptions {
        CallOptions { only: self.only.clone(), except: self.except.clone(), editing: false }
    }
}

/// OffenseLimit: how many offenses each cop gets to report, counted across a request.
struct Limit {
    /// The maximum, and whether the client sent it as a float (json_schemer takes `2.0` as an
    /// integer, and Ruby then counts with the Float).
    max: (f64, bool),
    offered: Vec<(&'static str, usize)>,
}

impl Limit {
    fn new(max: (f64, bool)) -> Limit {
        Limit { max, offered: Vec::new() }
    }

    /// A validated `max_offenses_per_cop`.
    fn max(value: &Json) -> Option<(f64, bool)> {
        match value {
            Json::Num(n) => n.parse().ok().map(|v| (v, n.contains(['.', 'e', 'E']))),
            _ => None,
        }
    }

    /// OffenseLimit#filter, an offense at a time.
    fn admit(&mut self, cop: &'static str) -> bool {
        let i = match self.offered.iter().position(|(c, _)| *c == cop) {
            Some(i) => i,
            None => {
                self.offered.push((cop, 0));
                self.offered.len() - 1
            }
        };
        self.offered[i].1 += 1;
        self.offered[i].1 as f64 <= self.max.0
    }

    /// OffenseLimit#elided_per_cop, sorted by cop name, as JSON.
    fn elided(&self) -> Option<String> {
        let mut elided: Vec<(&str, f64)> = self
            .offered
            .iter()
            .filter(|(_, n)| *n as f64 > self.max.0)
            .map(|(c, n)| (*c, *n as f64 - self.max.0))
            .collect();
        if elided.is_empty() {
            return None;
        }
        elided.sort_by(|a, b| a.0.cmp(b.0));
        // ponytail: Float#to_s via Debug, right for the small whole numbers a count leaves.
        let count = |n: f64| if self.max.1 { format!("{n:?}") } else { format!("{}", n as i64) };
        let pairs: Vec<String> = elided.iter().map(|(c, n)| format!("{}:{}", json(c), count(*n))).collect();
        Some(format!("{{{}}}", pairs.join(",")))
    }
}

/// One file of a result: its `--format json` entry, with an autocorrection's `corrected` and `diff`.
struct Entry {
    path: String,
    offenses: Vec<String>,
    /// JSONFormatter#hash_for_problems' keys as JSON (`,"errors":[...]`), and the two counts.
    problems: (String, usize, usize),
    corrected: Option<bool>,
    diff: Option<String>,
}

impl Entry {
    /// Server#nothing_to_report?
    fn nothing_to_report(&self) -> bool {
        self.offenses.is_empty() && self.corrected != Some(true) && self.problems.0.is_empty()
    }

    fn to_json(&self) -> String {
        let mut s = format!("{{\"path\":{}", json(&self.path));
        if let Some(c) = self.corrected {
            s.push_str(&format!(",\"corrected\":{c}"));
        }
        if let Some(d) = &self.diff {
            s.push_str(&format!(",\"diff\":{}", json(d)));
        }
        s.push_str(&format!(",\"offenses\":[{}]{}}}", self.offenses.join(","), self.problems.0));
        s
    }
}

/// Server#build_result's hash: the files to report and the summary's keys in order.
struct Outcome {
    files: Vec<Entry>,
    summary: Vec<(&'static str, String)>,
}

/// Server#build_result and #build_summary: files with nothing to report are only counted.
fn build_result(targets: usize, entries: Vec<Entry>) -> Outcome {
    let offense_count: usize = entries.iter().map(|e| e.offenses.len()).sum();
    let mut summary = vec![("target_file_count", targets.to_string()), ("offense_count", offense_count.to_string())];
    // JSONFormatter#problem_counts: only the non-zero ones.
    let errors: usize = entries.iter().map(|e| e.problems.1).sum();
    let warnings: usize = entries.iter().map(|e| e.problems.2).sum();
    for (key, n) in [("error_count", errors), ("warning_count", warnings)] {
        if n > 0 {
            summary.push((key, n.to_string()));
        }
    }
    Outcome { files: entries.into_iter().filter(|e| !e.nothing_to_report()).collect(), summary }
}

/// Server#result_json: the agent never sees stderr, so the summary says what the limit left out.
fn result_json(result: &mut Outcome, limit: Option<&Limit>) -> String {
    if let Some(elided) = limit.and_then(Limit::elided) {
        result.summary.push(("unreported_offenses", elided));
    }
    let files: Vec<String> = result.files.iter().map(Entry::to_json).collect();
    let summary: Vec<String> = result.summary.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    format!("{{\"files\":[{}],\"summary\":{{{}}}}}", files.join(","), summary.join(","))
}

/// One property of a tool's input schema, as json_schemer checks it.
#[derive(Clone, Copy)]
enum Kind {
    Str,
    Bool,
    /// `{type: 'integer', minimum: 1}`
    PositiveInt,
    /// `{type: 'array', items: {type: 'string'}}`, with its `minItems`.
    Strings(usize),
    /// `{type: %w[boolean string]}`
    BoolOrStr,
}

impl Kind {
    /// json_schemer's errors for `value` at `pointer`, in the schema's keyword order.
    fn errors(self, pointer: &str, value: &Json) -> Vec<String> {
        let not = |what: &str| format!("value at `{pointer}` is not {what}");
        match (self, value) {
            (Kind::Str, Json::Str(_))
            | (Kind::Bool, Json::Bool(_))
            | (Kind::BoolOrStr, Json::Bool(_) | Json::Str(_)) => Vec::new(),
            (Kind::Str, _) => vec![not("a string")],
            (Kind::Bool, _) => vec![not("a boolean")],
            (Kind::BoolOrStr, _) => vec![not("one of the types: [\"boolean\", \"string\"]")],
            (Kind::PositiveInt, Json::Num(n)) => {
                let v: f64 = n.parse().unwrap_or(0.0);
                let mut errors = Vec::new();
                if v.fract() != 0.0 {
                    errors.push(not("an integer"));
                }
                if v < 1.0 {
                    errors.push(format!("number at `{pointer}` is less than: 1"));
                }
                errors
            }
            (Kind::PositiveInt, _) => vec![not("an integer")],
            (Kind::Strings(min), Json::Arr(items)) => {
                let mut errors: Vec<String> = items
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| !matches!(v, Json::Str(_)))
                    .map(|(i, _)| format!("value at `{pointer}/{i}` is not a string"))
                    .collect();
                if items.len() < min {
                    errors.push(format!("array size at `{pointer}` is less than: {min}"));
                }
                errors
            }
            (Kind::Strings(_), _) => vec![not("an array")],
        }
    }
}

/// Each tool's input schema properties, in order (build_tool's, then Scope::PROPERTIES).
fn schema(tool: &str) -> &'static [(&'static str, Kind)] {
    const INSPECTION: [(&str, Kind); 6] = [
        ("path", Kind::Str),
        ("source_code", Kind::Str),
        ("max_offenses_per_cop", Kind::PositiveInt),
        ("only", Kind::Strings(0)),
        ("except", Kind::Strings(0)),
        ("changed", Kind::BoolOrStr),
    ];
    const AUTOCORRECTION: [(&str, Kind); 9] = [
        ("path", Kind::Str),
        ("source_code", Kind::Str),
        ("safety", Kind::Bool),
        ("contextual", Kind::Bool),
        ("dry_run", Kind::Bool),
        ("max_offenses_per_cop", Kind::PositiveInt),
        ("only", Kind::Strings(0)),
        ("except", Kind::Strings(0)),
        ("changed", Kind::BoolOrStr),
    ];
    const EXPLAIN: [(&str, Kind); 2] = [("cop_names", Kind::Strings(1)), ("path", Kind::Str)];
    match tool {
        "rubocop_inspection" => &INSPECTION,
        "rubocop_autocorrection" => &AUTOCORRECTION,
        _ => &EXPLAIN,
    }
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

/// JSONFormatter#hash_for_problems' messages for one key, as plain strings: what a crash in
/// corrected inline code fails the call with.
fn problem_messages(abs: &Path, cwd: &Path, errors: &[String]) -> Vec<String> {
    let (fragment, _, _) = rbcop::formatter::hash_for_problems(&smart_path(abs, cwd), abs, errors, &[]);
    let Some(list) = fragment.strip_prefix(',') else { return Vec::new() };
    Json::parse(&format!("{{{list}}}"))
        .map_or(Vec::new(), |j| j.get("errors").map_or(Vec::new(), |e| e.items().iter().map(Json::string).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_cursors() {
        assert!(valid_id(&Json::Num("3".into())) && !valid_id(&Json::Num("1.5".into())));
        assert!(valid_id(&Json::Str("ab-1".into())) && !valid_id(&Json::Str("a b".into())));
        assert_eq!(list(None, &TOOLS).unwrap().matches("\"name\"").count(), 3);
        assert!(list(Some(&Json::Obj(vec![("cursor".into(), Json::Str("3".into()))])), &TOOLS).is_err());
    }
}
