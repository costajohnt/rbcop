//! Run the enabled cops over one file: parse once, walk once, dispatch nodes by kind.

use crate::autocorrect::{self, Mode, corrector::Corrector};
use crate::config::{CopConfig, Resolved, target_ruby};
use crate::cop::{Cop, Cx, Facts, Fixes, Heredoc, Offense, Severity};
use crate::directives;
use crate::helpers::comment_config::{StatementBounds, code_lines};
use crate::node_kind;
use crate::source::{Comment, Source};
use ruby_prism::{
    ArgumentsNode, BlockArgumentNode, BlockNode, BlockParameterNode, CallNode, ConstantPathNode, ElseNode, EnsureNode,
    InterpolatedStringNode, LocalVariableTargetNode, Node, ParametersNode, RegularExpressionNode, RescueNode,
    SplatNode, StatementsNode, StringNode, Visit, XStringNode,
};
use std::path::Path;
use std::sync::Arc;

/// What happened to one file.
pub struct FileReport {
    pub offenses: Vec<Offense>,
    /// Source text (after CRLF normalisation), kept for formatters that print code.
    pub text: String,
}

impl FileReport {
    /// The source an offense's offsets refer to (autocorrect keeps each iteration's source).
    pub fn text_of<'a>(&'a self, o: &'a Offense) -> &'a str {
        o.source.as_deref().unwrap_or(&self.text)
    }
}

struct Active<'c> {
    name: &'static str,
    cop: &'static dyn Cop,
    cfg: &'c CopConfig,
}

/// Inspect one file. `selected` filters cop names (from --only / --except).
pub fn inspect(
    path: &Path,
    display: &str,
    text: String,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
) -> FileReport {
    // parser's Source::Buffer normalises CRLF before any cop sees the source; the raw text stays
    // available (ProcessedSource#raw_source) for cops such as Layout/EndOfLine.
    let (text, raw) = if text.contains("\r\n") { (text.replace("\r\n", "\n"), Some(text)) } else { (text, None) };
    let shared: Arc<str> = Arc::from(text.as_str());
    let (offenses, _, _) = analyze(path, display, &shared, raw.as_deref(), cfg, selected, None, Redundant::Own);
    FileReport { offenses, text }
}

/// Inspect a file's bytes as read. A source that must be UTF-8 (`source::must_be_utf8`) but is not
/// gets only RuboCop's Lint/Syntax `Invalid byte sequence in utf-8.` (a US-ASCII one with a byte
/// above 0x7f its transcoding error, `source::encoding_error`); other non-UTF-8 sources are
/// inspected lossily.
pub fn inspect_bytes(
    path: &Path,
    display: &str,
    bytes: Vec<u8>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
) -> FileReport {
    let error = crate::source::encoding_error(&bytes);
    match String::from_utf8(bytes) {
        Ok(text) => match error {
            Some(msg) => encoding_error_report(text, msg),
            None => inspect(path, display, text, cfg, selected),
        },
        Err(e) => {
            let text = String::from_utf8_lossy(e.as_bytes()).into_owned();
            if let Some(msg) = error {
                return encoding_error_report(text, msg);
            }
            if !crate::source::must_be_utf8(&text) {
                return inspect(path, display, text, cfg, selected);
            }
            encoding_error_report(text, "Invalid byte sequence in utf-8.".into())
        }
    }
}

/// Lint/Syntax#add_offense_from_error: a global offense, beautified message, no version note.
fn encoding_error_report(text: String, message: String) -> FileReport {
    let offense = Offense {
        start: 0,
        end: 0,
        cop: "Lint/Syntax",
        message,
        severity: Severity::Fatal,
        correctable: false,
        global: true,
        corrected: false,
        source: None,
    };
    FileReport { offenses: vec![offense], text }
}

/// How Ruby words a failed `File.read`/`File.open` of `path` (RuboCop raises its own error for a
/// missing file).
pub fn io_error(e: &std::io::Error, path: &Path) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        return format!("Error: No such file or directory: {}", path.display());
    }
    let msg = e.to_string();
    let msg = msg.split(" (os error").next().unwrap_or(&msg);
    format!("{msg} @ rb_sysopen - {}", path.display())
}

/// One autocorrect iteration (see `crate::autocorrect::correct`): the offenses and, unless nothing
/// was corrected, the collated corrector. Like Team#investigate_with_corrector, offenses of cops
/// that do not autocorrect are only reported when no correction was made. Also returns the
/// (cop, first line, last line) of the offenses disabled by comments, which RuboCop keeps for
/// Lint/RedundantCopDisableDirective.
pub fn analyze_for_correction(
    path: &Path,
    display: &str,
    text: &Arc<str>,
    raw_text: Option<&str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
) -> (Vec<Offense>, Option<Corrector>, Vec<Span>) {
    analyze(path, display, text, raw_text, cfg, selected, Some(mode), Redundant::Skip)
}

/// Runner#add_redundant_disables after an autocorrect loop: Lint/RedundantCopDisableDirective
/// alone, judging the directives of `text` against `checked` (every offense the loop found).
pub fn redundant_disables(
    path: &Path,
    display: &str,
    text: &Arc<str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
    checked: &[Span],
) -> (Vec<Offense>, Option<Corrector>) {
    let (offenses, corrector, _) =
        analyze(path, display, text, None, cfg, selected, Some(mode), Redundant::Only(checked));
    (offenses, corrector)
}

/// An offense as Lint/RedundantCopDisableDirective sees it: cop, first line, last line.
pub type Span = (&'static str, usize, usize);

/// When Lint/RedundantCopDisableDirective runs (it needs every other cop's offenses).
#[derive(Clone, Copy)]
enum Redundant<'a> {
    /// After this file's cops (plain inspection).
    Own,
    /// Not in this pass (autocorrect iterations).
    Skip,
    /// Only it, against offenses found earlier.
    Only(&'a [Span]),
}

const REDUNDANT_DISABLE: &str = "Lint/RedundantCopDisableDirective";

#[allow(clippy::too_many_arguments)]
fn analyze(
    path: &Path,
    display: &str,
    shared: &Arc<str>,
    raw_text: Option<&str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Option<Mode>,
    redundant: Redundant,
) -> (Vec<Offense>, Option<Corrector>, Vec<Span>) {
    let text: &str = shared;
    let raw_text = raw_text.unwrap_or(text);
    let version = target_ruby::prism_version(cfg.target_ruby);
    // Below 3.3 RuboCop parses with the parser gem, which accepts some things Prism rejects.
    let parser_gem = cfg.target_ruby < 3.3;
    let (mut result, tokens) = if parser_gem {
        ruby_prism::parse_with_version_and_tokens(text.as_bytes(), version, !crate::source::binary_encoding(text))
    } else {
        (ruby_prism::parse_with_version(text.as_bytes(), version, !crate::source::binary_encoding(text)), Vec::new())
    };
    // Numbered parameters are 2.7+: below that `foo { _1 }` is a block without arguments.
    if cfg.target_ruby < 2.7 {
        result.drop_numbered_parameters();
    }
    let mut errors: Vec<(usize, usize, String)> = result
        .errors()
        .filter(|e| !tolerated(e, parser_gem, cfg.target_ruby, text))
        .map(|e| {
            let loc = e.location();
            crate::syntax::translate(e.type_name(), e.message(), loc.start_offset(), loc.end_offset(), text)
        })
        .collect();
    // ...rejects syntax newer than the target, and reports where its grammar fails rather than
    // Prism's messages (semantic errors keep their translated parser-gem messages).
    if parser_gem {
        let kept = |e: &ruby_prism::Diagnostic| !tolerated(e, parser_gem, cfg.target_ruby, text);
        // A stray backslash is the parser gem's lexer error: reported, then skipped (no recovery).
        let bare_backslash = |e: &ruby_prism::Diagnostic| {
            let loc = e.location();
            e.type_name() == "unexpected_token_ignore" && text.get(loc.start_offset()..loc.end_offset()) == Some("\\")
        };
        // `@`, `@@` or `$` without a name: the lexer's fatal "unexpected `@'" ends the parse there.
        let fatal = |e: &ruby_prism::Diagnostic| {
            let s = e.location().start_offset();
            let after = text[s..].trim_start_matches(['@', '$']).chars().next();
            match e.type_name().trim_end_matches("_3_3") {
                "incomplete_variable_instance" | "incomplete_variable_class" => {
                    !after.is_some_and(|c| c.is_ascii_digit())
                }
                "invalid_variable_global"
                | "instance_variable_bare"
                | "class_variable_bare"
                | "global_variable_bare" => true,
                _ => false,
            }
        };
        let fatal_at = result.errors().filter(|e| kept(e) && fatal(e)).map(|e| e.location().start_offset()).min();
        // A string left open at the end of the file: the lexer's fatal string_eof, whatever the
        // parser reported before.
        let open_string = result
            .errors()
            .filter(|e| e.type_name() == "string_literal_eof")
            .map(|e| e.location().start_offset())
            .min();
        let semantic_kind = |e: &ruby_prism::Diagnostic| {
            (crate::syntax::is_semantic(e.type_name()) && e.type_name() != "string_literal_eof") || bare_backslash(e)
        };
        let seed = result
            .errors()
            .filter(|e| kept(e) && !semantic_kind(e) && !fatal(e) && e.type_name() != "string_literal_eof")
            .map(|e| {
                crate::legacy_syntax::failure_offset(
                    e.type_name(),
                    e.location().start_offset(),
                    e.location().end_offset(),
                    &tokens,
                )
            })
            .min();
        // The parser gem reports semantic errors as it reduces, so only those before its first
        // grammar failure on Prism's syntax error.
        let cut = seed.unwrap_or(usize::MAX);
        let mut actions: Vec<usize> = Vec::new();
        // Circular argument references: the simulation reports them where the parser reads them.
        let mut circular: Vec<(usize, usize)> = Vec::new();
        let semantic: Vec<_> = result
            .errors()
            .filter(|e| kept(e) && semantic_kind(e) && !fatal(e) && e.location().start_offset() < cut)
            // `class` with no name (Prism reports the keyword): the parser fails before a name
            // reduces, so there is no name to check.
            .filter(|e| {
                !(matches!(e.type_name(), "class_name" | "module_name")
                    && tokens.iter().any(|t| t.start == e.location().start_offset() && t.kind.starts_with("KEYWORD_")))
            })
            .filter_map(|e| {
                let (s, end) = crate::legacy_syntax::semantic_location(
                    e.type_name(),
                    e.location().start_offset(),
                    e.location().end_offset(),
                    &tokens,
                    text,
                );
                if bare_backslash(&e) {
                    return Some((s, end, "bare backslash only allowed before newline".to_string()));
                }
                if e.type_name() == "parameter_circular" {
                    circular.push((s, end));
                    return None;
                }
                if e.type_name() == "regexp_parse_error"
                    && let Some(t) = crate::legacy_syntax::invalid_regexp(e.message(), s, &tokens, text)
                {
                    actions.push(t.1);
                    return Some(t);
                }
                let kind = crate::syntax::parser_gem_kind(e.type_name());
                let t = crate::syntax::translate(kind, e.message(), s, end, text);
                actions.push(t.1);
                Some(t)
            })
            .collect();
        actions.sort_unstable();
        let diags = crate::legacy_syntax::diagnostics(
            &result.node(),
            &tokens,
            text,
            cfg.target_ruby,
            seed,
            &actions,
            &circular,
        );
        // Where racc gave up (an empty message): the lexer never gets further.
        let stopped = diags.iter().find(|d| d.2.is_empty()).map(|d| d.0);
        let lexed = |o: &usize| stopped.is_none_or(|s| *o <= s);
        let (fatal_at, open_string) = (fatal_at.filter(lexed), open_string.filter(lexed));
        // A tolerated `<<''` (ended by the end of the file) counts only on a last line without a
        // newline, where the parser gem never starts the heredoc.
        let heredoc_term: Vec<usize> = result
            .errors()
            .filter(|e| e.type_name() == "heredoc_term")
            .filter(|e| kept(e) || !text[e.location().end_offset()..].contains('\n'))
            .map(|e| e.location().start_offset())
            .collect();
        let heredoc = crate::legacy_syntax::unterminated_heredoc(&tokens, text, &heredoc_term).filter(|h| lexed(&h.0));
        errors = diags
            .into_iter()
            .filter(|d| !d.2.is_empty())
            .map(|(s, e, msg)| crate::syntax::translate("", &msg, s, e, text))
            .chain(semantic)
            .collect();
        // Whichever stops the parser gem's lexer first (an open string before an `@` on its offset).
        let first = [open_string.map(|s| (s, 0)), fatal_at.map(|f| (f, 1)), heredoc.as_ref().map(|h| (h.0, 2))]
            .into_iter()
            .flatten()
            .min();
        match first {
            Some((f, 1)) => {
                errors.retain(|e| e.0 < f);
                errors.push((f, f + 1, format!("unexpected `{}'", text.get(f..f + 1).unwrap_or(""))));
            }
            Some((s, 0)) => {
                errors.retain(|e| e.0 < s);
                errors.push((s, s + 1, "unterminated string meets end of file".to_string()));
            }
            Some((s, _)) => {
                errors.retain(|e| e.0 < s);
                errors.extend(heredoc.into_iter().flat_map(|h| h.1));
            }
            None => {}
        }
    }
    // Pattern variable names the parser gem's builder rejects, only reached when Prism found no
    // error (a tree walk, so only on trees the depth check below accepts; patterns need `in`/`=>`).
    if errors.is_empty()
        && (text.contains("in") || text.contains("=>"))
        && !crate::ast::deeper_than(&result.node(), MAX_AST_DEPTH)
    {
        errors.extend(crate::syntax::pattern_variable_errors(&result.node()));
    }
    let comments: Vec<Comment> = result
        .comments()
        .map(|c| Comment {
            start: c.location().start_offset(),
            end: c.location().end_offset(),
            embdoc: c.type_() == ruby_prism::CommentType::EmbDocComment,
        })
        .collect();
    let data = result.data_loc().map(|d| d.start_offset());
    let mut src = Source::new(display, text, comments, data, version);
    src.raw_text = raw_text;

    if !errors.is_empty() {
        // RuboCop runs only Lint/Syntax on a file that does not parse, one offense per diagnostic,
        // and does so even under --only.
        let ruby = format_ruby_version(cfg.target_ruby);
        let mut out: Vec<Offense> = errors
            .into_iter()
            .map(|(start, end, msg)| Offense {
                start,
                end,
                cop: "Lint/Syntax",
                // Lint/Syntax#add_offense_from_diagnostic: no parser line under LSP (--editor-mode).
                message: if crate::helpers::lsp::enabled() {
                    msg
                } else {
                    format!(
                        "{msg}\n(Using Ruby {ruby} parser; configure using `TargetRubyVersion` parameter, under `AllCops`)"
                    )
                },
                severity: Severity::Fatal,
                correctable: false,
                global: false,
                corrected: false,
                source: None,
            })
            .collect();
        // The parser translation layer reports one diagnostic per range; Prism can stack two
        // (`unexpected '+'` and `expected a receiver for unary +`), and only the first survives.
        let mut seen = std::collections::HashSet::new();
        out.retain(|o| seen.insert((o.start, o.end)));
        // Then Runner's uniq (same position and message, any range), as for every cop.
        dedup(&mut out);
        return (out, None, Vec::new());
    }

    let opted_in = directives::opted_in(&src);
    let only_redundant = matches!(redundant, Redundant::Only(_));
    let active: Vec<Active> = crate::cops::ALL
        .iter()
        .filter(|r| selected(r.name) && !only_redundant)
        .filter_map(|r| {
            let c = cfg.cop(r.name);
            let on = c.enabled || opted_in.contains(&r.name);
            let (min, max) = r.cop.ruby_range();
            let ruby_ok = min <= cfg.target_ruby && cfg.target_ruby <= max;
            (on && ruby_ok && cfg.relevant(c, path)).then_some(Active { name: r.name, cop: r.cop, cfg: c })
        })
        .collect();

    let program = result.node();
    // Deliberate difference: RuboCop dies with SystemStackError on very deep nesting; rbcop skips the
    // file with an error instead, so cops (many of them recursive) never see such a tree.
    if crate::ast::deeper_than(&program, MAX_AST_DEPTH) {
        file_error(format!("rbcop: {}: nested more than {MAX_AST_DEPTH} levels deep, not inspected.", src.path));
        return (Vec::new(), None, Vec::new());
    }
    let mut facts = Facts::default();
    let mut collector = FactCollector { src: &src, facts: &mut facts };
    collector.facts.string_lines = vec![false; src.line_count() + 2];
    collector.visit(&program);
    let pn = program.as_program_node().unwrap();
    let first_code = pn.statements().body().iter().next().map(|n| n.location().start_offset());
    facts.first_code = first_code;
    // Registry#disabled_names of the --only/--except filtered registry.
    let config_disabled: Vec<&'static str> = crate::helpers::cop_registry::REGISTRY
        .iter()
        .copied()
        .filter(|&c| selected(c) && !directives::config_enabled(cfg, c))
        .collect();
    if src.text.contains("rubocop") {
        // DisableNext needs statement bounds before the cops run (disabled ranges gate `add_fix`).
        let bounds = directives::needs_statement_bounds(&src).then(|| StatementBounds::build(&src, &program));
        let bounds = bounds.unwrap_or_default();
        let code = code_lines(&src, &facts);
        let statement_bounds = |line: usize| bounds.at(line);
        let info = directives::LineInfo { code: &code, statement_bounds: &statement_bounds };
        let cc = directives::analyze(&src, &config_disabled, &info, directives::prevent_style(cfg));
        facts.disabled = directives::disabled_ranges(&cc);
        facts.comment_config = cc;
        facts.code_lines = code;
        facts.config_disabled = config_disabled;
    }
    facts.text = shared.clone();
    let mut fixes: Vec<Fixes> = active
        .iter()
        .map(|a| Fixes {
            autocorrect: mode.is_some_and(|m| a.cop.correctable() && autocorrect::cop_autocorrects(a.cfg, m)),
            ..Fixes::default()
        })
        .collect();

    let mut dispatch: Vec<Vec<usize>> = vec![Vec::new(); node_kind::COUNT];
    for (i, a) in active.iter().enumerate() {
        for &k in a.cop.kinds() {
            dispatch[k].push(i);
        }
    }
    let mut out = Vec::new();
    let mut states: Vec<Option<Box<dyn std::any::Any>>> = (0..active.len()).map(|_| None).collect();
    let mut walker = Walker {
        src: &src,
        facts: &facts,
        config: cfg,
        active: &active,
        dispatch: &dispatch,
        stack: Vec::new(),
        leaf: None,
        out: &mut out,
        states: &mut states,
        fixes: &mut fixes,
    };
    walker.visit(&program);

    for ((a, state), fixes) in active.iter().zip(states.iter_mut()).zip(fixes.iter_mut()) {
        let mut cx = Cx {
            src: &src,
            facts: &facts,
            ancestors: &[],
            cfg: a.cfg,
            config: cfg,
            cop: a.name,
            correctable: a.cop.correctable(),
            out: &mut out,
            state,
            fixes,
        };
        guarded(a.name, &src, 0, || a.cop.on_file(&mut cx));
    }

    dedup(&mut out);
    // Parser's Range#last_line is the line of end_pos.
    let span = |o: &Offense| (o.cop, src.line_of(o.start), src.line_of(o.end.max(o.start)));
    let mut disabled: Vec<Span> = Vec::new();
    out.retain(|o| {
        let s = span(o);
        let off = directives::is_disabled(&facts.disabled, s.0, s.1, s.2);
        if off {
            disabled.push(s);
        }
        !off
    });

    // Runner#add_redundant_disables: offenses_to_check are all offenses, the disabled ones included.
    let checked: Option<Vec<Span>> = match redundant {
        Redundant::Own if mode.is_none() => Some(out.iter().map(span).chain(disabled.iter().copied()).collect()),
        Redundant::Only(c) => Some(c.to_vec()),
        _ => None,
    };
    let mut redundant_fix = None;
    let rcfg = cfg.cop(REDUNDANT_DISABLE);
    if let Some(checked) = checked
        && selected(REDUNDANT_DISABLE)
        && (rcfg.enabled || opted_in.contains(&REDUNDANT_DISABLE))
        && cfg.relevant(rcfg, path)
        && (!facts.disabled.is_empty() || !facts.comment_config.detached.is_empty())
    {
        let mut fixes =
            Fixes { autocorrect: mode.is_some_and(|m| autocorrect::cop_autocorrects(rcfg, m)), ..Fixes::default() };
        let (mut found, mut state) = (Vec::new(), None);
        let mut cx = Cx {
            src: &src,
            facts: &facts,
            ancestors: &[],
            cfg: rcfg,
            config: cfg,
            cop: REDUNDANT_DISABLE,
            correctable: true,
            out: &mut found,
            state: &mut state,
            fixes: &mut fixes,
        };
        guarded(REDUNDANT_DISABLE, &src, 0, || {
            crate::cops::lint_redundant_cop_disable_directive::check(&mut cx, &checked)
        });
        found.retain(|o| {
            let s = span(o);
            !directives::is_disabled(&facts.disabled, s.0, s.1, s.2)
        });
        out.extend(found);
        redundant_fix = fixes.corrector;
    }

    let mut corrector = None;
    if mode.is_some() {
        let mut by_cop: Vec<(&'static str, Corrector)> =
            active.iter().zip(fixes).filter_map(|(a, f)| Some((a.name, f.corrector?))).collect();
        by_cop.extend(redundant_fix.map(|c| (REDUNDANT_DISABLE, c)));
        corrector = autocorrect::collate(by_cop, shared.clone());
        if corrector.is_some() && !only_redundant {
            // Team#investigate_with_corrector: the other cops do not run in a correcting iteration.
            let correcting: Vec<&str> = active
                .iter()
                .filter(|a| a.cop.correctable() && autocorrect::cop_autocorrects(a.cfg, mode.unwrap()))
                .map(|a| a.name)
                .collect();
            out.retain(|o| correcting.contains(&o.cop));
        }
    }
    for o in out.iter_mut() {
        let c = cfg.cop(o.cop);
        o.severity = c.severity.unwrap_or_else(|| default_severity(o.cop));
        o.correctable &= autocorrect_allowed(c);
    }
    (out, corrector, disabled)
}

thread_local! {
    static IN_COP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runner#errors: every cop error of the run, for the end-of-run summary and the exit status.
static ERRORS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Print a cop error now (Commissioner) and keep it for the summary.
pub fn cop_error(cop: &str, src: &Source, at: usize) {
    // Range#line and the 0-based Range#column.
    let (line, col) = crate::formatter::line_col(src.text, at);
    file_error(format!("An error occurred while {cop} cop was inspecting {}:{line}:{}.", src.path, col - 1));
}

/// Print an error now and keep it for the summary (it makes the run fail).
pub fn file_error(msg: String) {
    eprintln!("{msg}");
    ERRORS.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
}

/// AST depth beyond which a file is not inspected. Real code stays under 50 (the deepest of the
/// rails, discourse and RuboCop corpora is 46). At this depth every cop, inspecting and correcting,
/// ran in the default 2 MiB thread stack; main.rs gives worker threads 64 MiB of headroom.
pub const MAX_AST_DEPTH: usize = 1000;

/// The run's cop errors, in file order.
pub fn take_errors() -> Vec<String> {
    let mut errors = std::mem::take(&mut *ERRORS.lock().unwrap_or_else(|e| e.into_inner()));
    errors.sort();
    errors.dedup();
    errors
}

/// Whether this thread is running a cop callback (its panics are reported by `guarded`).
pub fn in_cop() -> bool {
    IN_COP.with(|c| c.get())
}

/// Commissioner#with_cop_error_handling: a cop that fails on one node is reported and skipped
/// there, and inspection goes on (other nodes, other cops, other files).
fn guarded(cop: &str, src: &Source, at: usize, f: impl FnOnce()) {
    IN_COP.with(|c| c.set(true));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    IN_COP.with(|c| c.set(false));
    if result.is_err() {
        cop_error(cop, src, at);
    }
}

/// Cop::Base#use_corrector: an offense with a corrector is only `[Correctable]` when the cop's
/// `AutoCorrect` is `always`/`true` (the default) or `contextual` (not under `--editor-mode`);
/// `false`/`disabled` turn it off.
fn autocorrect_allowed(cfg: &CopConfig) -> bool {
    use crate::config::value::Value;
    match cfg.get("AutoCorrect") {
        None | Some(Value::Bool(true)) => true,
        Some(Value::Str(s)) => s == "always" || (s == "contextual" && !crate::helpers::lsp::enabled()),
        _ => false,
    }
}

/// Prism reports some semantic errors the parser gem accepts (e.g. top-level `yield` in .builder
/// templates, `retry` outside `rescue`).
// ponytail: only verified ones are tolerated; add more as real code turns them up.
const TOLERATED_ERRORS: &[&str] = &["invalid_yield", "invalid_retry_without_rescue"];

/// Prism errors the parser gem accepts for the target. Up to Ruby 2.5, `begin..else..end` without
/// `rescue` is only a `useless_else` warning (reported by Lint/UselessElseWithoutRescue); before
/// 2.7 `def m(x = x)` is only a warning (Lint/CircularArgumentReference); before 3.0 `_1 = x` is
/// accepted (Builders::Default#check_reserved_for_numparam; Lint/NumberedParameterAssignment).
/// The parser gem also ends a `<<''` heredoc (empty delimiter) at the end of the file, where Prism
/// finds it unterminated.
fn tolerated(e: &ruby_prism::Diagnostic, parser_gem: bool, target_ruby: f64, text: &str) -> bool {
    let kind = e.type_name();
    (parser_gem && TOLERATED_ERRORS.contains(&kind))
        || (kind == "begin_lonely_else" && target_ruby <= 2.5)
        || (kind == "parameter_circular" && target_ruby < 2.7)
        || (kind == "parameter_numbered_reserved" && target_ruby < 3.0)
        || (parser_gem && e.message().starts_with("unterminated heredoc; can't find string \"\"") && {
            // No blank line (which ends the parser gem's heredoc) before the end of the file.
            let body = text[e.location().end_offset()..].split_once('\n').map_or("", |(_, b)| b);
            body.lines().all(|l| !l.trim().is_empty())
        })
}

/// Cop::Base#default_severity.
pub fn default_severity(cop: &str) -> Severity {
    if cop == "Lint/Syntax" {
        Severity::Fatal
    } else if cop.starts_with("Lint/") || cop.starts_with("Security/") {
        // Cop::Base::DEPARTMENT_SEVERITIES
        Severity::Warning
    } else if cop.starts_with("Metrics/") {
        Severity::Refactor
    } else {
        Severity::Convention
    }
}

fn same(a: &Node, b: &Node) -> bool {
    let (la, lb) = (a.location(), b.location());
    node_kind::of(a) == node_kind::of(b) && la.start_offset() == lb.start_offset() && la.end_offset() == lb.end_offset()
}

struct Walker<'s, 'pr, 'c> {
    src: &'s Source<'s>,
    facts: &'s Facts,
    config: &'s Resolved,
    active: &'s [Active<'c>],
    dispatch: &'s [Vec<usize>],
    stack: Vec<Node<'pr>>,
    leaf: Option<Node<'pr>>,
    out: &'s mut Vec<Offense>,
    states: &'s mut [Option<Box<dyn std::any::Any>>],
    fixes: &'s mut [Fixes],
}

impl<'pr> Walker<'_, 'pr, '_> {
    fn dispatch(&mut self, node: &Node<'pr>) {
        for &i in &self.dispatch[node_kind::of(node)] {
            let a = &self.active[i];
            let mut cx = Cx {
                src: self.src,
                facts: self.facts,
                ancestors: &self.stack,
                cfg: a.cfg,
                config: self.config,
                cop: a.name,
                correctable: a.cop.correctable(),
                out: self.out,
                state: &mut self.states[i],
                fixes: &mut self.fixes[i],
            };
            guarded(a.name, self.src, node.location().start_offset(), || a.cop.on_node(&mut cx, node));
        }
    }

    /// For node types Prism's default visitor reaches without the enter/leave hooks.
    fn branch(&mut self, node: Node<'pr>, walk: impl FnOnce(&mut Self)) {
        if self.stack.last().is_some_and(|top| same(top, &node)) {
            walk(self);
        } else {
            self.visit_branch_node_enter(node);
            walk(self);
            self.visit_branch_node_leave();
        }
    }

    fn leaf(&mut self, node: Node<'pr>) {
        if !self.leaf.as_ref().is_some_and(|l| same(l, &node)) {
            self.dispatch(&node);
        }
    }
}

impl<'pr> Visit<'pr> for Walker<'_, 'pr, '_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.dispatch(&node);
        self.stack.push(node);
    }

    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }

    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.dispatch(&node);
        self.leaf = Some(node);
    }

    fn visit_leaf_node_leave(&mut self) {
        self.leaf = None;
    }

    fn visit_arguments_node(&mut self, node: &ArgumentsNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_arguments_node(s, node));
    }
    fn visit_block_argument_node(&mut self, node: &BlockArgumentNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_block_argument_node(s, node));
    }
    fn visit_block_node(&mut self, node: &BlockNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_block_node(s, node));
    }
    fn visit_block_parameter_node(&mut self, node: &BlockParameterNode<'pr>) {
        self.leaf(node.as_node());
    }
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_call_node(s, node));
    }
    fn visit_constant_path_node(&mut self, node: &ConstantPathNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_constant_path_node(s, node));
    }
    fn visit_else_node(&mut self, node: &ElseNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_else_node(s, node));
    }
    fn visit_ensure_node(&mut self, node: &EnsureNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_ensure_node(s, node));
    }
    fn visit_local_variable_target_node(&mut self, node: &LocalVariableTargetNode<'pr>) {
        self.leaf(node.as_node());
    }
    fn visit_parameters_node(&mut self, node: &ParametersNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_parameters_node(s, node));
    }
    fn visit_rescue_node(&mut self, node: &RescueNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_rescue_node(s, node));
    }
    fn visit_splat_node(&mut self, node: &SplatNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_splat_node(s, node));
    }
    fn visit_statements_node(&mut self, node: &StatementsNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_statements_node(s, node));
    }
}

/// Collects `Facts` (string and heredoc layout) in a separate, cheap walk.
struct FactCollector<'s, 'a> {
    src: &'s Source<'a>,
    facts: &'s mut Facts,
}

impl FactCollector<'_, '_> {
    fn cover(&mut self, start: usize, end: usize) {
        if end > start {
            for l in self.src.line_of(start)..=self.src.line_of(end - 1) {
                self.facts.string_lines[l] = true;
            }
        }
    }

    fn string_like(&mut self, start: usize, end: usize, opening: Option<&[u8]>, body: Option<(usize, usize, usize)>) {
        let Some(open) = opening else { return };
        if !open.starts_with(b"<<") {
            self.facts.string_ranges.push((start, end));
        } else if let Some((b, t, t_end)) = body {
            self.heredoc(b, t, t_end, open);
            self.facts.string_ranges.push((b, t));
        }
    }

    /// Body from `body` to the terminator line starting at `term` (ending at `term_end`).
    fn heredoc(&mut self, body: usize, term: usize, term_end: usize, opening: &[u8]) {
        let delimiter = self.src.slice(term, term_end).trim().to_string();
        self.facts.heredocs.push(Heredoc {
            first: self.src.line_of(body),
            terminator: self.src.line_of(term),
            delimiter,
            quoted: opening.ends_with(b"'"),
        });
    }
}

impl<'pr> Visit<'pr> for FactCollector<'_, '_> {
    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
        let loc = node.location();
        let body = node.closing_loc().map(|t| (c.start_offset(), t.start_offset(), t.end_offset()));
        self.string_like(loc.start_offset(), loc.end_offset(), node.opening_loc().map(|o| o.as_slice()), body);
    }

    fn visit_interpolated_string_node(&mut self, node: &InterpolatedStringNode<'pr>) {
        let loc = node.location();
        let body = match (node.parts().first(), node.closing_loc()) {
            (Some(p), Some(t)) => Some((p.location().start_offset(), t.start_offset(), t.end_offset())),
            _ => None,
        };
        self.string_like(loc.start_offset(), loc.end_offset(), node.opening_loc().map(|o| o.as_slice()), body);
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_x_string_node(&mut self, node: &XStringNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
        if node.opening_loc().as_slice().starts_with(b"<<") {
            let t = node.closing_loc();
            self.heredoc(c.start_offset(), t.start_offset(), t.end_offset(), node.opening_loc().as_slice());
        }
    }

    fn visit_regular_expression_node(&mut self, node: &RegularExpressionNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
    }
}

/// Ruby's Float#to_s for a version (3.4, 2.7, 4.0).
fn format_ruby_version(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// Cop::Base#add_offense ignores a second offense on a range the cop already reported.
/// Runner's `offenses_by_iteration.flatten.uniq`: Offense equality is line, column, cop, message and
/// severity (fixed per cop), not the range, so two ranges starting at one position collapse.
fn dedup(out: &mut Vec<Offense>) {
    let mut seen = std::collections::HashSet::new();
    out.retain(|o| seen.insert((o.cop, o.start, o.message.clone())));
}
