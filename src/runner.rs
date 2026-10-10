//! Run the enabled cops over one file: parse once, walk once, dispatch nodes by kind.

use crate::config::{CopConfig, Resolved, target_ruby};
use crate::cop::{Cop, Cx, Facts, Heredoc, Offense, Severity};
use crate::directives;
use crate::node_kind;
use crate::source::{Comment, Source};
use ruby_prism::{
    ArgumentsNode, BlockArgumentNode, BlockNode, BlockParameterNode, CallNode, ConstantPathNode, ElseNode, EnsureNode,
    InterpolatedStringNode, LocalVariableTargetNode, Node, ParametersNode, RegularExpressionNode, RescueNode,
    SplatNode, StatementsNode, StringNode, Visit, XStringNode,
};
use std::path::Path;

/// What happened to one file.
pub struct FileReport {
    pub offenses: Vec<Offense>,
    /// Source text (after CRLF normalisation), kept for formatters that print code.
    pub text: String,
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
    // parser's Source::Buffer normalises CRLF before any cop sees the source.
    let text = if text.contains("\r\n") { text.replace("\r\n", "\n") } else { text };
    let offenses = analyze(path, display, &text, cfg, selected);
    FileReport { offenses, text }
}

fn analyze(path: &Path, display: &str, text: &str, cfg: &Resolved, selected: &dyn Fn(&str) -> bool) -> Vec<Offense> {
    let version = target_ruby::prism_version(cfg.target_ruby);
    let result = ruby_prism::parse_with_version(text.as_bytes(), version);
    // Below 3.3 RuboCop parses with the parser gem, which accepts some things Prism rejects.
    let parser_gem = cfg.target_ruby < 3.3;
    let errors: Vec<(usize, usize, String)> = result
        .errors()
        .filter(|e| !(parser_gem && TOLERATED_ERRORS.contains(&e.type_name())))
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
    let src = Source::new(display, text, comments, data, version);

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
            })
            .collect();
        dedup(&mut out);
        return out;
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

    let mut dispatch: Vec<Vec<usize>> = vec![Vec::new(); node_kind::COUNT];
    for (i, a) in active.iter().enumerate() {
        for &k in a.cop.kinds() {
            dispatch[k].push(i);
        }
    }
    let mut out = Vec::new();
    let mut walker = Walker {
        src: &src,
        facts: &facts,
        config: cfg,
        active: &active,
        dispatch: &dispatch,
        stack: Vec::new(),
        leaf: None,
        out: &mut out,
    };
    walker.visit(&program);

    for a in &active {
        let mut cx = Cx {
            src: &src,
            facts: &facts,
            ancestors: &[],
            cfg: a.cfg,
            config: cfg,
            cop: a.name,
            correctable: a.cop.correctable(),
            out: &mut out,
        };
        a.cop.on_file(&mut cx);
    }

    let config_disabled: Vec<&'static str> = opted_in.iter().copied().filter(|c| !cfg.cop(c).enabled).collect();
    let disabled = directives::disabled_spans(&src, &config_disabled);
    dedup(&mut out);
    out.retain(|o| !directives::is_disabled(&disabled, o.cop, src.line_of(o.start)));
    for o in out.iter_mut() {
        o.severity = cfg.cop(o.cop).severity.unwrap_or_else(|| default_severity(o.cop));
    }
    out
}

/// Prism reports some semantic errors the parser gem accepts (e.g. top-level `yield` in .builder templates).
// ponytail: only verified ones are tolerated; add more as real code turns them up.
const TOLERATED_ERRORS: &[&str] = &["invalid_yield"];

/// Cop::Base#default_severity.
pub fn default_severity(cop: &str) -> Severity {
    if cop == "Lint/Syntax" {
        Severity::Fatal
    } else if cop.starts_with("Lint/") {
        Severity::Warning
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
            };
            a.cop.on_node(&mut cx, node);
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
            self.heredoc(b, t, t_end);
            self.facts.string_ranges.push((b, t));
        }
    }

    /// Body from `body` to the terminator line starting at `term` (ending at `term_end`).
    fn heredoc(&mut self, body: usize, term: usize, term_end: usize) {
        let delimiter = self.src.slice(term, term_end).trim().to_string();
        self.facts.heredocs.push(Heredoc {
            first: self.src.line_of(body),
            terminator: self.src.line_of(term),
            delimiter,
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
            self.heredoc(c.start_offset(), t.start_offset(), t.end_offset());
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
fn dedup(out: &mut Vec<Offense>) {
    let mut seen = std::collections::HashSet::new();
    out.retain(|o| seen.insert((o.cop, o.start, o.end)));
}
