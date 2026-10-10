//! The interface every cop implements, and the context it reports through.

use crate::config::{CopConfig, Resolved};
use crate::source::Source;
use ruby_prism::Node;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
}

/// A heredoc's body lines, `first..terminator` (1-based), and its closing delimiter.
#[derive(Clone, Debug)]
pub struct Heredoc {
    pub first: usize,
    pub terminator: usize,
    pub delimiter: String,
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
        self.push(start, end, message.into(), self.correctable);
    }

    /// Report an offense whose `[Correctable]` flag differs from the cop's default
    /// (e.g. LineLength is only correctable when it finds a place to break the line).
    pub fn add_with(&mut self, start: usize, end: usize, message: impl Into<String>, correctable: bool) {
        self.push(start, end, message.into(), correctable);
    }

    fn push(&mut self, start: usize, end: usize, message: String, correctable: bool) {
        self.out.push(Offense { start, end, cop: self.cop, message, severity: Severity::Convention, correctable });
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
