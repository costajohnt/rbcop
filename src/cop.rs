//! The interface every cop implements, and the context it reports through.

pub use crate::autocorrect::corrector::Corrector;
use crate::config::{CopConfig, Resolved};
use crate::source::Source;
use ruby_prism::Node;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Refactor,
    Convention,
    Warning,
    Error,
    Fatal,
}

impl Severity {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "info" => Severity::Info,
            "refactor" => Severity::Refactor,
            "convention" => Severity::Convention,
            "warning" => Severity::Warning,
            "error" => Severity::Error,
            "fatal" => Severity::Fatal,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Refactor => "refactor",
            Severity::Convention => "convention",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }

    /// Single-letter code used by the emacs/clang/simple formatters.
    pub fn code(self) -> char {
        self.name().chars().next().unwrap().to_ascii_uppercase()
    }
}

#[derive(Clone, Debug)]
pub struct Offense {
    /// Byte range in the source.
    pub start: usize,
    pub end: usize,
    pub cop: &'static str,
    pub message: String,
    pub severity: Severity,
    pub correctable: bool,
    /// From `add_global_offense`: no location (Offense::NO_LOCATION). `start`/`end` are 0 so the
    /// emacs-style formats print 1:1; formatters that show source lines print none.
    pub global: bool,
    /// Fixed by autocorrect (`[Corrected]`); always false without `-a`/`-A`.
    pub corrected: bool,
    /// The source the offsets refer to when it is not `FileReport::text` (autocorrect reports
    /// offenses from every iteration, each against the source that iteration inspected).
    pub source: Option<Arc<str>>,
    /// Suppressed by a directive comment (status `:disabled`); kept only under `--display-suppressed`.
    pub disabled: bool,
    /// DirectiveComment#reason of the directive that suppressed it (the text after `--`).
    pub justification: Option<String>,
    /// `--disable-uncorrectable` gave it a `# rubocop:todo` comment (status `:corrected_with_todo`).
    pub todo: bool,
    /// Under `--lsp`, or with the JSON formatter (`record_edits`): Offense#corrector's replacements
    /// (byte ranges), for the quick fix and Offense#corrections; None when the cop made no
    /// correction.
    pub edits: Option<Vec<(usize, usize, String)>>,
}

static RECORD_EDITS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Keep every offense's correction in `Offense::edits` (the JSON formatter prints them).
pub fn record_edits() {
    RECORD_EDITS.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn recording_edits() -> bool {
    RECORD_EDITS.load(std::sync::atomic::Ordering::Relaxed) || crate::helpers::lsp::serving()
}

/// A heredoc's body lines, `first..terminator` (1-based), and its closing delimiter.
#[derive(Clone, Debug)]
pub struct Heredoc {
    pub first: usize,
    pub terminator: usize,
    pub delimiter: String,
    /// Opening delimiter in single quotes (`<<~'EOS'`): no interpolation.
    pub quoted: bool,
}

/// Facts about the file gathered before cops run, for cops that work on lines.
#[derive(Default)]
pub struct Facts {
    pub heredocs: Vec<Heredoc>,
    /// Lines that hold string, regexp or xstring content (the parser gem emits a token per such line).
    pub string_lines: Vec<bool>,
    /// Byte ranges of delimited str/dstr literals and of heredoc bodies.
    pub string_ranges: Vec<(usize, usize)>,
    /// Byte offset of the first non-comment token, if any.
    pub first_code: Option<usize>,
    /// (cop, first line, last line) spans switched off by `# rubocop:disable` comments.
    pub disabled: std::collections::HashMap<String, Vec<(i64, i64)>>,
    /// The full directive analysis behind `disabled` (empty unless the file mentions `rubocop`).
    pub comment_config: crate::directives::CommentConfig,
    /// `code_lines[line]`: a non-comment token starts on that line (only computed when the file
    /// mentions `rubocop`).
    pub code_lines: Vec<bool>,
    /// Registry#disabled_names of the --only/--except filtered registry (only computed when the
    /// file mentions `rubocop`).
    pub config_disabled: Vec<&'static str>,
    /// The file text shared by correctors.
    pub text: Arc<str>,
    /// Under `--disable-uncorrectable`: the ranges of heredocs, backslash-continued strings, percent
    /// arrays and multi-line dstrs, in AST pre-order (AutocorrectLogic#multiline_ranges).
    pub literal_ranges: Vec<(usize, usize)>,
}

/// Autocorrect state of the running cop for one file (RuboCop's per-cop `current_corrector`).
#[derive(Default)]
pub struct Fixes {
    /// `Cop::Base#autocorrect?`: `-a`/`-A` given, and the cop's corrections are enabled (and safe for `-a`).
    pub autocorrect: bool,
    /// Corrections of this cop's offenses so far, merged.
    pub corrector: Option<Corrector>,
    /// Ranges reported so far (`current_offense_locations`).
    pub seen: HashSet<(usize, usize)>,
    /// `--disable-uncorrectable`: an offense this run does not correct gets a `# rubocop:todo`
    /// comment (the cop autocorrects, or its unsafe correction is skipped under `-a`).
    pub todo: bool,
    /// Lines given a todo comment so far (`currently_disabled_lines`).
    pub todo_lines: HashSet<usize>,
}

/// What a cop sees while it runs on one node or one file.
pub struct Cx<'s, 'pr> {
    pub src: &'s Source<'s>,
    pub facts: &'s Facts,
    /// Ancestors of the current node, outermost first (Prism nodes, so wrappers such as
    /// StatementsNode appear; see `crate::ast` for parser-gem-shaped views).
    pub ancestors: &'s [Node<'pr>],
    pub cfg: &'s CopConfig,
    /// The whole resolved config, for cops that read other cops' settings (`config.for_cop(...)`).
    pub config: &'s Resolved,
    pub cop: &'static str,
    /// `Cop::correctable()` of the running cop.
    pub correctable: bool,
    pub out: &'s mut Vec<Offense>,
    /// Per-file scratch state of the running cop (cops themselves are shared and immutable).
    pub state: &'s mut Option<Box<dyn std::any::Any>>,
    /// Autocorrect state of the running cop.
    pub fixes: &'s mut Fixes,
}

impl Cx<'_, '_> {
    /// The running cop's per-file state, created with `Default` on first use. It persists from
    /// the first `on_node` call through `on_file` for the same file.
    pub fn state<T: Default + 'static>(&mut self) -> &mut T {
        if !self.state.as_ref().is_some_and(|s| s.is::<T>()) {
            *self.state = Some(Box::new(T::default()));
        }
        self.state.as_mut().unwrap().downcast_mut::<T>().unwrap()
    }

    /// Report an offense on a byte range. Severity is filled in by the runner.
    pub fn add(&mut self, start: usize, end: usize, message: impl Into<String>) {
        self.add_with(start, end, message, self.correctable);
    }

    /// Report an offense whose `[Correctable]` flag differs from the cop's default
    /// (e.g. LineLength is only correctable when it finds a place to break the line).
    pub fn add_with(&mut self, start: usize, end: usize, message: impl Into<String>, correctable: bool) {
        // Base#add_offense: `return unless current_offense_locations.add?(range)`, whatever the
        // message, so a cop's second offense on an identical range is dropped.
        if self.fixes.seen.contains(&(start, end)) {
            return;
        }
        if self.fixes.todo && !self.suppressed(start, end) {
            return self.push_todo(start, end, message.into());
        }
        self.push(start, end, message.into(), correctable, false);
    }

    fn suppressed(&self, start: usize, end: usize) -> bool {
        crate::directives::suppressed(&self.facts.disabled, self.cop, self.src.line_of(start), self.src.line_of(end))
    }

    /// Base#disable_uncorrectable: a `# rubocop:todo` comment for the offense (none if its line has
    /// one from this cop already), status `:corrected_with_todo`.
    fn push_todo(&mut self, start: usize, end: usize, message: String) {
        let mut edits = None;
        if self.fixes.todo_lines.insert(self.src.line_of(start)) {
            let mut c = Corrector::new(self.facts.text.clone());
            crate::autocorrect::disable_offense(
                &mut c,
                self.src,
                &self.facts.literal_ranges,
                self.config,
                self.cop,
                start,
                end,
            );
            let merged = self.fixes.corrector.get_or_insert_with(|| Corrector::new(self.facts.text.clone()));
            if merged.merge(&c).is_err() {
                return self.cop_error();
            }
            edits = (recording_edits() && !c.is_empty()).then(|| c.as_replacements());
        }
        self.push(start, end, message, true, true);
        if let Some(o) = self.out.last_mut() {
            o.todo = true;
            o.edits = edits;
        }
    }

    /// Report an offense with no location (RuboCop's `add_global_offense`), e.g. a file name problem.
    pub fn add_global(&mut self, message: impl Into<String>) {
        // Base#add_global_offense asks the comment config too (enabled_line?).
        if !crate::directives::ignoring_disable_comments() && !crate::runner::skipping_offenses() {
            crate::runner::comment_config_read();
        }
        let message = message.into();
        self.out.push(Offense {
            start: 0,
            end: 0,
            cop: self.cop,
            message,
            severity: Severity::Convention,
            correctable: false,
            global: true,
            corrected: false,
            source: None,
            disabled: false,
            justification: None,
            todo: false,
            edits: None,
        });
    }

    /// Report an offense with a corrector block, like `add_offense(range) { |corrector| ... }`.
    /// The block runs unless the range is disabled by a comment or was already reported; the
    /// offense is `[Correctable]` only if the block made an edit (and AutoCorrect allows it).
    /// Under `-a`/`-A` the edits are applied and the offense reported `[Corrected]`.
    pub fn add_fix(&mut self, start: usize, end: usize, message: impl Into<String>, fix: impl FnOnce(&mut Corrector)) {
        if !self.fixes.seen.insert((start, end)) {
            return;
        }
        let (first, last) = (self.src.line_of(start), self.src.line_of(end));
        if crate::directives::suppressed(&self.facts.disabled, self.cop, first, last) {
            // Reported as :disabled (the runner drops it); the block never runs.
            return self.push(start, end, message.into(), false, false);
        }
        let mut corrector = Corrector::new(self.facts.text.clone());
        fix(&mut corrector);
        if corrector.clobbered() {
            return self.cop_error();
        }
        // Base#use_corrector: under --disable-uncorrectable an offense without a correction (or
        // whose unsafe correction -a skips) gets a todo comment instead.
        if self.fixes.todo && (corrector.is_empty() || !self.fixes.autocorrect) {
            return self.push_todo(start, end, message.into());
        }
        if corrector.is_empty() {
            return self.push(start, end, message.into(), false, false);
        }
        let edits = recording_edits().then(|| corrector.as_replacements());
        if self.fixes.autocorrect {
            // Base#apply_correction: merged into the cop's corrector; clobbering there raises too.
            let merged = self.fixes.corrector.get_or_insert_with(|| Corrector::new(self.facts.text.clone()));
            if merged.merge(&corrector).is_err() {
                return self.cop_error();
            }
        }
        self.push(start, end, message.into(), true, self.fixes.autocorrect);
        if let Some(o) = self.out.last_mut() {
            o.edits = edits;
        }
    }

    /// An offense RuboCop corrects (so `[Correctable]`) whose correction rbcop deliberately withholds:
    /// the text it would insert comes from string/symbol content that is not inert Ruby
    /// (docs/known-gaps.md, "Deliberate differences"). The source is left untouched.
    pub fn add_withheld_fix(&mut self, start: usize, end: usize, message: impl Into<String>) {
        if !self.fixes.seen.insert((start, end)) {
            return;
        }
        let (first, last) = (self.src.line_of(start), self.src.line_of(end));
        let correctable = !crate::directives::suppressed(&self.facts.disabled, self.cop, first, last);
        self.push(start, end, message.into(), correctable, false);
    }

    /// A corrector raised ClobberingError: RuboCop reports the cop error (at the node the cop was
    /// visiting, not the offense) and loses the offense.
    fn cop_error(&self) {
        crate::runner::cop_error_at(self.cop, self.src, crate::runner::visiting());
    }

    fn push(&mut self, start: usize, end: usize, message: String, correctable: bool, corrected: bool) {
        // Base#add_offense asks the comment config whether the lines are enabled.
        if !crate::directives::ignoring_disable_comments() && !crate::runner::skipping_offenses() {
            crate::runner::poison_raise_offense(self.cop, start, &message);
        }
        self.fixes.seen.insert((start, end));
        self.out.push(Offense {
            start,
            end,
            cop: self.cop,
            message,
            severity: Severity::Convention,
            correctable,
            corrected,
            source: None,
            disabled: false,
            justification: None,
            global: false,
            todo: false,
            edits: None,
        });
    }

    /// Nearest ancestor (the parent in Prism's tree).
    pub fn parent(&self) -> Option<&Node<'_>> {
        self.ancestors.last()
    }

    /// Whether the block of an `add_offense` on this range would run now: the range is new and
    /// its lines are not disabled by a comment. Ask before reporting the offense.
    pub fn block_runs(&self, start: usize, end: usize) -> bool {
        !self.fixes.seen.contains(&(start, end))
            && !crate::directives::suppressed(
                &self.facts.disabled,
                self.cop,
                self.src.line_of(start),
                self.src.line_of(end),
            )
    }

    /// Update this cop's `--auto-gen-config` state (crate::autogen).
    pub fn autogen(&self, f: impl FnOnce(&mut crate::autogen::CopState) + Send + 'static) {
        crate::autogen::record(self.cop, f);
    }

    /// ConfigurableEnforcedStyle#style_detected with the given Symbol styles (`ambiguous_style_detected`
    /// for several), for the style parameter `param`.
    pub fn style_detected_for(&self, param: &'static str, detected: &[&str]) {
        if crate::autogen::on() {
            let d: Vec<crate::autogen::Style> = detected.iter().map(|s| Some(s.to_string())).collect();
            self.autogen(move |c| c.style_detected(param, &d));
        }
    }

    /// `style_detected` for `EnforcedStyle`.
    pub fn style_detected(&self, detected: &[&str]) {
        self.style_detected_for("EnforcedStyle", detected);
    }

    /// `unexpected_style_detected 'name'` with a String: SYMBOL_TO_STRING_CACHE maps it to nil.
    pub fn string_style_detected_for(&self, param: &'static str) {
        self.autogen(move |c| c.style_detected(param, &[None]));
    }

    /// `correct_style_detected` (the configured style) for `param`.
    pub fn correct_style_detected_for(&self, param: &'static str) {
        if crate::autogen::on()
            && let Some(style) = self.cfg.str(param)
        {
            let style = style.to_string();
            self.style_detected_for(param, &[&style]);
        }
    }

    pub fn correct_style_detected(&self) {
        self.correct_style_detected_for("EnforcedStyle");
    }

    /// `opposite_style_detected` for `param`: the other of exactly two supported styles.
    pub fn opposite_style_detected_for(&self, param: &'static str) {
        if crate::autogen::on() {
            // Util.to_supported_styles
            let supported = param.replacen("Enforced", "Supported", 1).replacen("Style", "Styles", 1);
            let style = self.cfg.str(param).unwrap_or_default().to_string();
            if let Some(other) = self.cfg.strings(&supported).into_iter().find(|s| *s != style) {
                self.style_detected_for(param, &[&other]);
            }
        }
    }

    pub fn opposite_style_detected(&self) {
        self.opposite_style_detected_for("EnforcedStyle");
    }

    /// `no_acceptable_style!` (also `unrecognized_style_detected`, `conflicting_styles_detected`).
    pub fn no_acceptable_style(&self) {
        self.autogen(|c| c.no_acceptable_style_now());
    }

    /// `add_offense(range, message:) { self.<param> = value }`: the ExcludeLimit value is recorded
    /// when the block runs.
    pub fn add_with_limit(
        &mut self,
        start: usize,
        end: usize,
        message: impl Into<String>,
        param: &'static str,
        value: i64,
    ) {
        if crate::autogen::on() && self.block_runs(start, end) {
            self.exclude_limit(param, value);
        }
        self.add(start, end, message);
    }

    /// ExcludeLimit: `self.<param> = value`.
    pub fn exclude_limit(&self, param: &'static str, value: i64) {
        self.autogen(move |c| c.limit(param, value));
    }
}

pub trait Cop: Send + Sync {
    /// Node kinds (`crate::node_kind::*`) this cop wants passed to `on_node`.
    fn kinds(&self) -> &'static [usize] {
        &[]
    }

    /// Called for every node of a subscribed kind, in source (pre-)order.
    fn on_node(&self, _cx: &mut Cx, _node: &Node) {}

    /// Called once per file after the AST walk (RuboCop's on_new_investigation / on_investigation_end).
    fn on_file(&self, _cx: &mut Cx) {}

    /// Whether RuboCop marks this cop's offenses `[Correctable]` by default (it has an autocorrector).
    fn correctable(&self) -> bool {
        false
    }

    /// `minimum_target_ruby_version` / `maximum_target_ruby_version`: the cop only runs when the
    /// target Ruby (TargetRubyVersion) is within this inclusive range.
    fn ruby_range(&self) -> (f64, f64) {
        (0.0, f64::MAX)
    }

    /// rubocop-rails `minimum_target_rails_version`: the cop only runs when the target Rails
    /// (`Resolved::target_rails_version`) is at least this.
    fn min_rails(&self) -> f64 {
        0.0
    }
}

/// A registered cop: qualified name plus implementation. The list lives in `crate::cops::ALL`.
pub struct Registered {
    pub name: &'static str,
    pub cop: &'static dyn Cop,
}
