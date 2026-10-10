//! Pieces of RuboCop's ReparsedEquivalence mixin: does a correction still parse, and does the
//! corrected code parse to the same (normalized) AST. Parser-gem AST equality is approximated by a
//! structural serialization of the Prism tree (kinds, call names, leaf source), normalized like
//! RedundantParentheses#normalize_reparsed_ast: one-child parentheses unwrapped, nested statement
//! sequences spliced, `and`/`or` chains flattened.

use crate::autocorrect::corrector::Corrector;
use crate::node_kind as kind;
use ruby_prism::{Node, Visit};
use std::sync::Arc;

/// `correction_parses?`: the source with the edits applied (no clobbering) has no syntax errors.
pub fn correction_parses(source: Arc<str>, version: &'static str, edit: impl FnOnce(&mut Corrector)) -> bool {
    let mut c = Corrector::new(source);
    edit(&mut c);
    if c.clobbered() {
        return false;
    }
    valid_syntax(&c.rewrite(), version)
}

pub fn valid_syntax(text: &str, version: &'static str) -> bool {
    ruby_prism::parse_with_version(text.as_bytes(), version).errors().next().is_none()
}

/// The normalized structure of `node` (whose source is `text`).
pub fn serialize(text: &str, node: &Node) -> String {
    let mut s = Ser { text, out: String::new(), frames: vec![] };
    s.visit(node);
    s.out
}

/// `parses_equivalently?`: `fragment` parses without errors to a single statement (or, with
/// `whole_file`, a program) whose structure is `original`.
pub fn parses_equivalently(original: &str, fragment: &str, whole_file: bool, version: &'static str) -> bool {
    let result = ruby_prism::parse_with_version(fragment.as_bytes(), version);
    if result.errors().next().is_some() {
        return false;
    }
    let root = result.node();
    if whole_file {
        return serialize(fragment, &root) == original;
    }
    let stmts: Vec<Node> = root.as_program_node().unwrap().statements().body().iter().collect();
    match stmts.as_slice() {
        [one] => serialize(fragment, one) == original,
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
}

struct Ser<'t> {
    text: &'t str,
    out: String,
    frames: Vec<Frame>,
}

impl Ser<'_> {
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
        if let Some(Frame::Open(_)) = self.frames.pop() {
            self.out.push(')');
        }
    }

    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        let loc = node.location();
        let k = match &node {
            Node::SourceLineNode { .. } => "__LINE__",
            // These span the whole block.
            Node::NumberedParametersNode { .. } | Node::ItParametersNode { .. } => "",
            _ => &self.text[loc.start_offset()..loc.end_offset()],
        };
        self.out.push('[');
        self.out.push_str(&kind::of(&node).to_string());
        self.out.push(' ');
        self.out.push_str(k);
        self.out.push(']');
    }

    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        let inline = self.last() == Some(Frame::Inline);
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

/// `reparse_scope`: the innermost def/class/module/sclass containing a range.
struct ScopeFinder<'pr> {
    range: (usize, usize),
    found: Option<Node<'pr>>,
}

impl<'pr> Visit<'pr> for ScopeFinder<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let loc = node.location();
        let scope = matches!(
            node,
            Node::DefNode { .. } | Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. }
        );
        if scope && loc.start_offset() <= self.range.0 && self.range.1 <= loc.end_offset() {
            self.found = Some(node);
        }
    }
}

/// (scope range, normalized original, items).
type Group<T> = (Option<(usize, usize)>, String, Vec<T>);

/// `verified_by_reparse(items, oversized: :verify)` with `apply_reparse_correction` as `apply`:
/// items grouped by reparse scope, a group kept whole when all its corrections together parse
/// equivalently, else item by item. `range` gives an item's range; returns the verified items.
pub fn verified_by_reparse<T: Copy>(
    text: &str,
    version: &'static str,
    program: &Node,
    items: &[T],
    range: impl Fn(T) -> (usize, usize),
    apply: impl Fn(&mut Corrector, T),
) -> Vec<T> {
    let source: Arc<str> = Arc::from(text);
    // (scope range, normalized original, items) in order of first appearance.
    let mut groups: Vec<Group<T>> = vec![];
    for &item in items {
        let mut f = ScopeFinder { range: range(item), found: None };
        f.visit(program);
        let scope = f.found.as_ref().map(|n| (n.location().start_offset(), n.location().end_offset()));
        match groups.iter_mut().find(|g| g.0 == scope) {
            Some(g) => g.2.push(item),
            None => {
                let original = serialize(text, f.found.as_ref().unwrap_or(program));
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
                corrected.get(s..end).is_some_and(|fragment| parses_equivalently(original, fragment, false, version))
            }
            None => parses_equivalently(original, &corrected, true, version),
        }
    };
    let mut out = vec![];
    for (scope, original, group) in &groups {
        if group.len() > 1 && verify(group, *scope, original) {
            out.extend(group.iter().copied());
        } else {
            out.extend(group.iter().copied().filter(|&i| verify(&[i], *scope, original)));
        }
    }
    out
}
