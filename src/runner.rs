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
    let (offenses, _) = analyze(path, display, &shared, raw.as_deref(), cfg, selected, None);
    FileReport { offenses, text }
}

/// One autocorrect iteration (see `crate::autocorrect::correct`): the offenses and, unless nothing
/// was corrected, the collated corrector. Like Team#investigate_with_corrector, offenses of cops
/// that do not autocorrect are only reported when no correction was made.
pub fn analyze_for_correction(
    path: &Path,
    display: &str,
    text: &Arc<str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
) -> (Vec<Offense>, Option<Corrector>) {
    analyze(path, display, text, None, cfg, selected, Some(mode))
}

fn analyze(
    path: &Path,
    display: &str,
    shared: &Arc<str>,
    raw_text: Option<&str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Option<Mode>,
) -> (Vec<Offense>, Option<Corrector>) {
    let text: &str = shared;
    let raw_text = raw_text.unwrap_or(text);
    let version = target_ruby::prism_version(cfg.target_ruby);
    let result = ruby_prism::parse_with_version(text.as_bytes(), version);
    // Below 3.3 RuboCop parses with the parser gem, which accepts some things Prism rejects.
    let parser_gem = cfg.target_ruby < 3.3;
    let errors: Vec<(usize, usize, String)> = result
        .errors()
        .filter(|e| !tolerated(e.type_name(), parser_gem, cfg.target_ruby))
        .map(|e| {
            let loc = e.location();
            crate::syntax::translate(e.type_name(), e.message(), loc.start_offset(), loc.end_offset(), text)
        })
        .collect();
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
                message: format!(
                    "{msg}\n(Using Ruby {ruby} parser; configure using `TargetRubyVersion` parameter, under `AllCops`)"
                ),
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
        return (out, None);
    }

    let opted_in = directives::opted_in(&src);
    let active: Vec<Active> = crate::cops::ALL
        .iter()
        .filter(|r| selected(r.name))
        .filter_map(|r| {
            let c = cfg.cop(r.name);
            let on = c.enabled || opted_in.contains(&r.name);
            let (min, max) = r.cop.ruby_range();
            let ruby_ok = min <= cfg.target_ruby && cfg.target_ruby <= max;
            (on && ruby_ok && cfg.relevant(c, path)).then_some(Active { name: r.name, cop: r.cop, cfg: c })
        })
        .collect();

    let program = result.node();
    let mut facts = Facts::default();
    let mut collector = FactCollector { src: &src, facts: &mut facts };
    collector.facts.string_lines = vec![false; src.line_count() + 2];
    collector.visit(&program);
    let pn = program.as_program_node().unwrap();
    let first_code = pn.statements().body().iter().next().map(|n| n.location().start_offset());
    facts.first_code = first_code;
    let config_disabled: Vec<&'static str> = opted_in.iter().copied().filter(|c| !cfg.cop(c).enabled).collect();
    if src.text.contains("rubocop") {
        // DisableNext needs statement bounds before the cops run (disabled ranges gate `add_fix`).
        let bounds = directives::needs_statement_bounds(&src).then(|| StatementBounds::build(&src, &program));
        let bounds = bounds.unwrap_or_default();
        let code = code_lines(&src, &facts);
        let statement_bounds = |line: usize| bounds.at(line);
        let info = directives::LineInfo { code: &code, statement_bounds: &statement_bounds };
        facts.disabled = directives::disabled_ranges(&src, &config_disabled, &info);
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
    out.retain(|o| {
        !directives::is_disabled(&facts.disabled, o.cop, src.line_of(o.start), src.line_of(o.end.max(o.start)))
    });
    let mut corrector = None;
    if mode.is_some() {
        let by_cop: Vec<(&'static str, Corrector)> =
            active.iter().zip(fixes).filter_map(|(a, f)| Some((a.name, f.corrector?))).collect();
        corrector = autocorrect::collate(by_cop, shared.clone());
        if corrector.is_some() {
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
    (out, corrector)
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
    let msg = format!("An error occurred while {cop} cop was inspecting {}:{line}:{}.", src.path, col - 1);
    eprintln!("{msg}");
    ERRORS.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
}

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
/// `AutoCorrect` is `always`/`true` (the default) or `contextual`; `false`/`disabled` turn it off.
fn autocorrect_allowed(cfg: &CopConfig) -> bool {
    use crate::config::value::Value;
    match cfg.get("AutoCorrect") {
        None | Some(Value::Bool(true)) => true,
        Some(Value::Str(s)) => s == "always" || s == "contextual",
        _ => false,
    }
}

/// Prism reports some semantic errors the parser gem accepts (e.g. top-level `yield` in .builder templates).
// ponytail: only verified ones are tolerated; add more as real code turns them up.
const TOLERATED_ERRORS: &[&str] = &["invalid_yield"];

/// Prism errors the parser gem accepts for the target. Up to Ruby 2.5, `begin..else..end` without
/// `rescue` is only a `useless_else` warning (reported by Lint/UselessElseWithoutRescue).
fn tolerated(kind: &str, parser_gem: bool, target_ruby: f64) -> bool {
    (parser_gem && TOLERATED_ERRORS.contains(&kind)) || (kind == "begin_lonely_else" && target_ruby <= 2.5)
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
