//! Pieces of RuboCop's ReparsedEquivalence mixin: does a correction still parse, and does the
//! corrected code parse to the same (normalized) AST. Parser-gem AST equality is approximated by a
//! structural serialization of the Prism tree (kinds, call names, leaf source), normalized like
//! RedundantParentheses#normalize_reparsed_ast: one-child parentheses unwrapped, nested statement
//! sequences spliced, `and`/`or` chains flattened.

use crate::autocorrect::corrector::Corrector;
use crate::node_kind as kind;
use crate::source::Source;
use ruby_prism::{Node, Visit};
use std::sync::Arc;

/// `correction_parses?`: the source with the edits applied (no clobbering) has no syntax errors.
pub fn correction_parses(
    source: Arc<str>,
    version: &'static str,
    legacy: Option<f64>,
    edit: impl FnOnce(&mut Corrector),
) -> bool {
    let mut c = Corrector::new(source);
    edit(&mut c);
    if c.clobbered() {
        return false;
    }
    valid_syntax(&c.rewrite(), version, legacy)
}

/// Prism parses `text` without errors, except those the parser gem accepts when it is the
/// target's parser (`retry` outside `rescue`, top-level `yield`; `Source::legacy`).
pub fn valid_syntax(text: &str, version: &'static str, legacy: Option<f64>) -> bool {
    parse_clean(text, version, legacy).is_some()
}

/// The parse of `text` when it has no syntax errors. With `legacy` (a target below 3.3, parsed by
/// the parser gem) Prism errors that gem accepts are ignored, and the target grammar's own
/// (`legacy_syntax`: newer syntax, `a = foo 1 or b`, ...) count.
fn parse_clean<'t>(text: &'t str, version: &'static str, legacy: Option<f64>) -> Option<ruby_prism::ParseResult<'t>> {
    let utf8 = !crate::source::binary_encoding(text);
    let Some(target) = legacy else {
        let result = ruby_prism::parse_with_version(text.as_bytes(), version, utf8);
        return (result.errors().count() == 0).then_some(result);
    };
    let (result, tokens) = ruby_prism::parse_with_version_and_tokens(text.as_bytes(), version, utf8);
    if result.errors().any(|e| !crate::runner::TOLERATED_ERRORS.contains(&e.type_name())) {
        return None;
    }
    let diags = crate::legacy_syntax::diagnostics(&result.node(), &tokens, text, target, None, &[], &[]);
    diags.is_empty().then_some(result)
}

/// The normalized structure of `node` (whose source is `text`).
pub fn serialize(text: &str, node: &Node) -> String {
    serialize_folding(text, node, false)
}

/// `serialize`, with RedundantLineBreak#fold_string_concatenation when `fold` is set: string
/// literals compare by value, adjacent literal parts merge, and `str + str` folds into one string.
pub fn serialize_folding(text: &str, node: &Node, fold: bool) -> String {
    let mut s = Ser { text, out: String::new(), frames: vec![], fold, suppress: 0, budget: FOLD_BUDGET };
    s.visit(node);
    s.out
}

/// `parses_equivalently?`: `fragment` parses without errors to a single statement (or, with
/// `whole_file`, a program) whose structure is `original`.
pub fn parses_equivalently(
    original: &str,
    fragment: &str,
    whole_file: bool,
    version: &'static str,
    legacy: Option<f64>,
) -> bool {
    parses_equivalently_folding(original, fragment, whole_file, version, legacy, false)
}

/// `parses_equivalently` with `serialize_folding`.
pub fn parses_equivalently_folding(
    original: &str,
    fragment: &str,
    whole_file: bool,
    version: &'static str,
    legacy: Option<f64>,
    fold: bool,
) -> bool {
    let Some(result) = parse_clean(fragment, version, legacy) else { return false };
    let root = result.node();
    if whole_file {
        return serialize_folding(fragment, &root, fold) == original;
    }
    let stmts: Vec<Node> = root.as_program_node().unwrap().statements().body().iter().collect();
    match stmts.as_slice() {
        [one] => serialize_folding(fragment, one, fold) == original,
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Frame {
    /// Emitted `(`...`)`.
    Open(usize),
    /// Unwrapped parentheses (their statements inline).
    Inline,
    /// A statements list.
    Stmts,
    /// The generic enter of a StatementsNode (a parentheses body): no output of its own.
    Pass,
    /// A string emitted folded: no output for anything below it.
    Folded,
}

/// A part of a folded string: literal bytes, or a serialized interpolation.
enum Part {
    Str(Vec<u8>),
    Other(String),
}

/// Fold steps per serialization: past this, strings stop folding (pathological `+` chains).
const FOLD_BUDGET: usize = 1_000_000;

struct Ser<'t> {
    text: &'t str,
    out: String,
    frames: Vec<Frame>,
    fold: bool,
    /// Nodes below a Folded frame produce no output.
    suppress: usize,
    budget: usize,
}

impl Ser<'_> {
    /// RedundantLineBreak#fold_string_concatenation of a node, when it folds to a str/dstr.
    fn folded(&mut self, node: &Node) -> Option<Vec<Part>> {
        let candidate = match node {
            Node::StringNode { .. } | Node::InterpolatedStringNode { .. } => true,
            Node::CallNode { .. } => node.as_call_node().unwrap().name().as_slice() == b"+",
            _ => false,
        };
        if !candidate {
            return None;
        }
        self.budget = self.budget.checked_sub(1)?;
        let parts: Vec<Part> = match node {
            Node::StringNode { .. } => return Some(vec![Part::Str(node.as_string_node().unwrap().unescaped().to_vec())]),
            Node::InterpolatedStringNode { .. } => {
                let mut out = vec![];
                for p in node.as_interpolated_string_node().unwrap().parts().iter() {
                    match self.folded(&p) {
                        Some(ps) => out.extend(ps),
                        None => {
                            let mut sub = Ser {
                                text: self.text,
                                out: String::new(),
                                frames: vec![],
                                fold: true,
                                suppress: 0,
                                budget: self.budget,
                            };
                            sub.visit(&p);
                            self.budget = sub.budget;
                            out.push(Part::Other(sub.out));
                        }
                    }
                }
                out
            }
            Node::CallNode { .. } => {
                let c = node.as_call_node().unwrap();
                if c.name().as_slice() != b"+" || crate::ast::is_csend(&c) || c.block().is_some() {
                    return None;
                }
                let mut out = self.folded(&c.receiver()?)?;
                for a in crate::ast::call_args(&c) {
                    out.extend(self.folded(&a)?);
                }
                out
            }
            _ => return None,
        };
        // merge_string_parts: adjacent literal parts join.
        let mut merged: Vec<Part> = vec![];
        for p in parts {
            match (merged.last_mut(), p) {
                (Some(Part::Str(a)), Part::Str(b)) => a.extend(b),
                (_, p) => merged.push(p),
            }
        }
        Some(merged)
    }

    fn emit_folded(&mut self, parts: &[Part]) {
        let str_of = |b: &[u8]| format!("[str {:?}]", String::from_utf8_lossy(b));
        if let [Part::Str(b)] = parts {
            self.out.push_str(&str_of(b));
            return;
        }
        self.out.push_str("(dstr");
        for p in parts {
            match p {
                Part::Str(b) => self.out.push_str(&str_of(b)),
                Part::Other(s) => self.out.push_str(s),
            }
        }
        self.out.push(')');
    }

    fn last(&self) -> Option<Frame> {
        self.frames.iter().rev().find(|f| **f != Frame::Pass).copied()
    }

    /// Kind of the nearest emitted frame (statement lists and unwrapped parentheses skipped).
    fn emitted_parent(&self) -> Option<usize> {
        self.frames.iter().rev().find_map(|f| match f {
            Frame::Open(k) => Some(*k),
            _ => None,
        })
    }
}

impl<'pr> Visit<'pr> for Ser<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        if self.suppress > 0 {
            return self.frames.push(Frame::Pass);
        }
        if self.fold
            && matches!(node, Node::InterpolatedStringNode { .. } | Node::CallNode { .. })
            && let Some(parts) = self.folded(&node)
        {
            self.emit_folded(&parts);
            self.suppress += 1;
            return self.frames.push(Frame::Folded);
        }
        let k = kind::of(&node);
        let frame = match &node {
            Node::StatementsNode { .. } => Frame::Pass,
            Node::ParenthesesNode { .. } => {
                let p = node.as_parentheses_node().unwrap();
                match p.body() {
                    None => Frame::Open(k),
                    Some(b) => match b.as_statements_node() {
                        Some(s) if s.body().iter().count() > 1 => {
                            // Spliced into an enclosing statement list.
                            if self.last() == Some(Frame::Stmts) { Frame::Inline } else { Frame::Open(k) }
                        }
                        _ => Frame::Inline,
                    },
                }
            }
            Node::AndNode { .. } | Node::OrNode { .. } if self.emitted_parent() == Some(k) => Frame::Inline,
            _ => Frame::Open(k),
        };
        if let Frame::Open(k) = frame {
            self.out.push('(');
            self.out.push_str(&k.to_string());
            if let Some(c) = node.as_call_node() {
                self.out.push(' ');
                // The method name, as in parser's `send`: an index call's message_loc spans its
                // arguments (`[("a")]`), and `foo.()` is `foo.call`.
                self.out.push_str(std::str::from_utf8(c.name().as_slice()).unwrap_or(""));
                self.out.push(if c.receiver().is_some() { 'r' } else { '-' });
                self.out
                    .push_str(c.call_operator_loc().map_or("", |m| std::str::from_utf8(m.as_slice()).unwrap_or("")));
            }
            if matches!(node, Node::PinnedExpressionNode { .. }) {
                // `^(a)` is parser `(pin (begin (lvar)))`, which normalizes to `^a`.
                self.out.truncate(self.out.len() - k.to_string().len());
                self.out.push_str(&kind::PINNED_VARIABLE_NODE.to_string());
            }
        }
        self.frames.push(frame);
    }

    fn visit_branch_node_leave(&mut self) {
        match self.frames.pop() {
            Some(Frame::Open(_)) => self.out.push(')'),
            Some(Frame::Folded) => self.suppress -= 1,
            _ => {}
        }
    }

    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        if self.suppress > 0 {
            return;
        }
        if self.fold
            && let Some(parts) = self.folded(&node)
        {
            return self.emit_folded(&parts);
        }
        // Implied by the `_1` reads; below 2.7 the parsed file has none (runner drops them).
        if matches!(node, Node::NumberedParametersNode { .. }) {
            return;
        }
        let loc = node.location();
        let k = match &node {
            Node::SourceLineNode { .. } => "__LINE__",
            // It spans the whole block.
            Node::ItParametersNode { .. } => "",
            _ => &self.text[loc.start_offset()..loc.end_offset()],
        };
        self.out.push('[');
        self.out.push_str(&kind::of(&node).to_string());
        self.out.push(' ');
        self.out.push_str(k);
        self.out.push(']');
    }

    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        let inline = self.suppress > 0 || self.last() == Some(Frame::Inline);
        if !inline {
            self.out.push('{');
        }
        self.frames.push(Frame::Stmts);
        ruby_prism::visit_statements_node(self, node);
        self.frames.pop();
        if !inline {
            self.out.push('}');
        }
    }
}

/// Every def/class/module/sclass, in pre-order (`reparse_scope` candidates).
#[derive(Default)]
struct Scopes<'pr>(Vec<Node<'pr>>);

impl<'pr> Visit<'pr> for Scopes<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        if matches!(
            node,
            Node::DefNode { .. } | Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. }
        ) {
            self.0.push(node);
        }
    }
}

/// (scope range, normalized original, items).
type Group<T> = (Option<(usize, usize)>, String, Vec<T>);

/// `verified_by_reparse(items, oversized: :verify)` with `apply_reparse_correction` as `apply`:
/// items grouped by reparse scope, a group kept whole when all its corrections together parse
/// equivalently, else item by item. `range` gives an item's range; returns the verified items.
pub fn verified_by_reparse<T: Copy>(
    src: &Source,
    program: &Node,
    items: &[T],
    range: impl Fn(T) -> (usize, usize),
    apply: impl Fn(&mut Corrector, T),
) -> Vec<T> {
    verified_by_reparse_folding(src, program, items, range, apply, false)
}

/// `verified_by_reparse` comparing with `serialize_folding(.., fold)` (RedundantLineBreak's
/// `normalize_reparsed_ast`).
pub fn verified_by_reparse_folding<T: Copy>(
    src: &Source,
    program: &Node,
    items: &[T],
    range: impl Fn(T) -> (usize, usize),
    apply: impl Fn(&mut Corrector, T),
    fold: bool,
) -> Vec<T> {
    let opts = ReparseOpts { fold, report_oversized: false, raw_text: src.raw_text };
    verified_by_reparse_opts(src.text, src.version, src.legacy(), program, items, range, apply, opts)
}

/// `corrected_scope_fragment`: `corrected[begin_pos...(end_pos + delta)]` in characters, where
/// RuboCop's delta is against the raw source, so each CRLF the normalised text lacks moves the
/// cut one character left (a negative end counts from the end, as Ruby's String#[] does).
/// `end` is the scope's end byte in `corrected`; `None` when Ruby's slice is nil or empty.
fn scope_fragment(corrected: &str, start: usize, end: usize, crlf: usize) -> Option<&str> {
    if crlf == 0 {
        return corrected.get(start..end);
    }
    let len = corrected.chars().count() as isize;
    let b = corrected.get(..start)?.chars().count() as isize;
    let mut x = corrected.get(..end)?.chars().count() as isize - crlf as isize;
    if x < 0 {
        x += len;
    }
    if x <= b {
        return None;
    }
    let byte = |c: isize| corrected.char_indices().nth(c as usize).map_or(corrected.len(), |(i, _)| i);
    Some(&corrected[start..byte(x.min(len))])
}

/// `verified_by_reparse(items, oversized:)`: with `report_oversized` (`oversized: :report`, the
/// default), a group whose scope (or the raw file) exceeds 64 KiB is kept without reparsing.
pub fn verified_by_reparse_with<T: Copy>(
    src: &Source,
    program: &Node,
    items: &[T],
    range: impl Fn(T) -> (usize, usize),
    apply: impl Fn(&mut Corrector, T),
    report_oversized: bool,
) -> Vec<T> {
    let opts = ReparseOpts { fold: false, report_oversized, raw_text: src.raw_text };
    verified_by_reparse_opts(src.text, src.version, src.legacy(), program, items, range, apply, opts)
}

/// MAX_VERIFICATION_FRAGMENT_SIZE (characters of the reparse scope).
const MAX_VERIFICATION_FRAGMENT_SIZE: usize = 64 * 1024;

/// Bound on item-by-item re-verification of a failed group: items x file bytes rewritten.
const MAX_ITEM_BY_ITEM_WORK: usize = 1 << 30;

#[derive(Clone, Copy)]
struct ReparseOpts<'a> {
    fold: bool,
    report_oversized: bool,
    /// `processed_source.raw_source` (CRLF intact): the whole-file size for `report_oversized`.
    raw_text: &'a str,
}

#[allow(clippy::too_many_arguments)]
fn verified_by_reparse_opts<T: Copy>(
    text: &str,
    version: &'static str,
    legacy: Option<f64>,
    program: &Node,
    items: &[T],
    range: impl Fn(T) -> (usize, usize),
    apply: impl Fn(&mut Corrector, T),
    opts: ReparseOpts,
) -> Vec<T> {
    let ReparseOpts { fold, report_oversized, raw_text } = opts;
    let crlf = if raw_text.len() == text.len() { 0 } else { raw_text.matches("\r\n").count() };
    let source: Arc<str> = Arc::from(text);
    // (scope range, normalized original, items) in order of first appearance.
    let mut groups: Vec<Group<T>> = vec![];
    let mut scopes = Scopes::default();
    if !items.is_empty() {
        scopes.visit(program);
    }
    for &item in items {
        // `reparse_scope`: the innermost (last in pre-order) scope containing the range.
        let r = range(item);
        let found = scopes.0.iter().rev().find(|n| {
            let loc = n.location();
            loc.start_offset() <= r.0 && r.1 <= loc.end_offset()
        });
        let scope = found.map(|n| (n.location().start_offset(), n.location().end_offset()));
        match groups.iter_mut().find(|g| g.0 == scope) {
            Some(g) => g.2.push(item),
            None => {
                let original = serialize_folding(text, found.unwrap_or(program), fold);
                groups.push((scope, original, vec![item]));
            }
        }
    }
    let verify = |group: &[T], scope: Option<(usize, usize)>, original: &str| {
        let mut c = Corrector::new(source.clone());
        for &item in group {
            apply(&mut c, item);
        }
        if c.clobbered() {
            return false;
        }
        let corrected = c.rewrite();
        match scope {
            Some((s, e)) => {
                let end = (e as isize + corrected.len() as isize - text.len() as isize) as usize;
                scope_fragment(&corrected, s, end, crlf)
                    .is_some_and(|fragment| parses_equivalently_folding(original, fragment, false, version, legacy, fold))
            }
            None => parses_equivalently_folding(original, &corrected, true, version, legacy, fold),
        }
    };
    let mut out = vec![];
    for (scope, original, group) in &groups {
        let too_large = || {
            let fragment = scope.map_or(raw_text, |(s, e)| &text[s..e]);
            fragment.chars().count() > MAX_VERIFICATION_FRAGMENT_SIZE
        };
        if (report_oversized && too_large()) || (group.len() > 1 && verify(group, *scope, original)) {
            out.extend(group.iter().copied());
        } else if group.len().saturating_mul(text.len()) <= MAX_ITEM_BY_ITEM_WORK {
            out.extend(group.iter().copied().filter(|&i| verify(&[i], *scope, original)));
        }
        // ponytail: past MAX_ITEM_BY_ITEM_WORK a failed group reports nothing (RuboCop would
        // rewrite and reparse once per item); deliberate bound for untrusted input.
    }
    out
}
