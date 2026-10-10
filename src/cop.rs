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
    /// The file text shared by correctors.
    pub text: Arc<str>,
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
        self.push(start, end, message.into(), self.correctable, false);
    }

    /// Report an offense whose `[Correctable]` flag differs from the cop's default
    /// (e.g. LineLength is only correctable when it finds a place to break the line).
    pub fn add_with(&mut self, start: usize, end: usize, message: impl Into<String>, correctable: bool) {
        self.push(start, end, message.into(), correctable, false);
    }

    /// Report an offense with no location (RuboCop's `add_global_offense`), e.g. a file name problem.
    pub fn add_global(&mut self, message: impl Into<String>) {
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
        if crate::directives::is_disabled(&self.facts.disabled, self.cop, first, last) {
            // Reported as :disabled (the runner drops it); the block never runs.
            return self.push(start, end, message.into(), false, false);
        }
        let mut corrector = Corrector::new(self.facts.text.clone());
        fix(&mut corrector);
        if corrector.clobbered() {
            return self.cop_error(start);
        }
        if corrector.is_empty() {
            return self.push(start, end, message.into(), false, false);
        }
        if !self.fixes.autocorrect {
            return self.push(start, end, message.into(), true, false);
        }
        // Base#apply_correction: merged into the cop's corrector; clobbering there raises too.
        let merged = self.fixes.corrector.get_or_insert_with(|| Corrector::new(self.facts.text.clone()));
        if merged.merge(&corrector).is_err() {
            return self.cop_error(start);
        }
        self.push(start, end, message.into(), true, true);
    }

    /// A corrector raised ClobberingError: RuboCop reports the cop error and loses the offense.
    fn cop_error(&self, start: usize) {
        let (line, col) = crate::formatter::line_col(self.src.text, start);
        eprintln!("An error occurred while {} cop was inspecting {}:{line}:{col}.", self.cop, self.src.path);
    }

    fn push(&mut self, start: usize, end: usize, message: String, correctable: bool, corrected: bool) {
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
            global: false,
        });
    }

    /// Nearest ancestor (the parent in Prism's tree).
    pub fn parent(&self) -> Option<&Node<'_>> {
        self.ancestors.last()
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
}

/// A registered cop: qualified name plus implementation. The list lives in `crate::cops::ALL`.
pub struct Registered {
    pub name: &'static str,
    pub cop: &'static dyn Cop,
}
