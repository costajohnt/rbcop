//! RuboCop's MultilineElementIndentation mixin (FirstArrayElementIndentation, FirstHashElementIndentation).

use crate::ast::{is_csend, send_args};
use crate::cop::Cx;
use crate::helpers::alignment::{corrector_edits, first_non_space, indentation_width};
use crate::node_kind as kind;
use crate::source::Source;
use ruby_prism::Node;

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// The `(` of the outermost call whose `on_send` would check this array/hash (via
/// `each_argument_node`): the literal sits in that call's arguments, its opening bracket is on the
/// paren's line, and no plain `send` lies between (`on_node(type, arg, :send)` does not descend
/// into sends; it does descend into csends and block bodies).
pub fn claiming_paren(src: &Source, node: &Node, ancestors: &[Node], left_brace: usize) -> Option<usize> {
    let mut child = span(node);
    let mut found = None;
    for a in ancestors.iter().rev() {
        if let Some(call) = a.as_call_node() {
            let via_block = call.block().is_some_and(|b| kind::of(&b) == kind::BLOCK_NODE && span(&b) == child);
            if !via_block {
                let in_args = send_args(&call).iter().any(|arg| span(arg) == child);
                if in_args
                    && let Some(open) = call.opening_loc().filter(|l| l.as_slice() == b"(")
                    && src.line_of(open.start_offset()) == src.line_of(left_brace)
                {
                    found = Some(open.start_offset());
                }
                if !is_csend(&call) {
                    break;
                }
            }
        }
        if kind::of(a) != kind::ARGUMENTS_NODE {
            child = span(a);
        }
    }
    found
}

pub enum IndentBase {
    LeftBrace,
    AfterParen,
    ParentHashKey,
    StartOfLine,
}

/// `indent_base(left_brace, first, left_parenthesis)`; `parent`/`grandparent` are the literal's
/// Prism parent (an AssocNode when the literal is a hash value) and that pair's hash.
pub fn indent_base(
    cx: &Cx,
    brace_style: &str,
    left_brace: usize,
    has_first: bool,
    left_paren: Option<usize>,
    parent: Option<&Node>,
    grandparent: Option<&Node>,
) -> (i64, IndentBase) {
    let src = cx.src;
    let style = cx.cfg.str("EnforcedStyle").unwrap_or("");
    if style == brace_style {
        return (src.column(left_brace) as i64, IndentBase::LeftBrace);
    }
    if has_first && let Some(pair) = parent.and_then(|p| p.as_assoc_node()) {
        let same = src.line_of(pair.key().location().start_offset()) == src.line_of(pair.value().location().start_offset());
        let elements: Vec<Node> = match grandparent {
            Some(g) => match (g.as_hash_node(), g.as_keyword_hash_node()) {
                (Some(h), _) => h.elements().iter().collect(),
                (_, Some(h)) => h.elements().iter().collect(),
                _ => vec![],
            },
            None => vec![],
        };
        let me = span(&pair.as_node());
        let sibling = elements.iter().position(|e| span(e) == me).and_then(|i| elements.get(i + 1));
        let next_line =
            sibling.is_some_and(|s| src.line_of(me.1.saturating_sub(1).max(me.0)) < src.line_of(span(s).0));
        if same && next_line {
            return (src.column(me.0) as i64, IndentBase::ParentHashKey);
        }
    }
    if let Some(p) = left_paren
        && style == "special_inside_parentheses"
    {
        return (src.column(p) as i64 + 1, IndentBase::AfterParen);
    }
    let line = src.lines.get(src.line_of(left_brace) - 1).copied().unwrap_or("");
    (line[..first_non_space(line).unwrap_or(0)].chars().count() as i64, IndentBase::StartOfLine)
}

/// Words for one literal kind.
pub struct Words {
    pub brace_style: &'static str,
    /// "array" / "hash"
    pub what: &'static str,
    /// "bracket" / "brace"
    pub bracket: &'static str,
    /// "opening bracket" / "opening brace"
    pub opening: &'static str,
    /// "left square bracket" / "left curly brace"
    pub left_name: &'static str,
}

pub fn first_message(cx: &Cx, w: &Words, base: &IndentBase) -> String {
    let desc = match base {
        IndentBase::LeftBrace => format!("the position of the {}", w.opening),
        IndentBase::AfterParen => "the first position after the preceding left parenthesis".to_string(),
        IndentBase::ParentHashKey => "the parent hash key".to_string(),
        IndentBase::StartOfLine => format!("the start of the line where the {} is", w.left_name),
    };
    format!("Use {} spaces for indentation in an {}, relative to {desc}.", indentation_width(cx), w.what)
        .replace("an hash", "a hash")
}

pub fn right_message(w: &Words, base: &IndentBase) -> String {
    let b = w.bracket;
    match base {
        IndentBase::LeftBrace => format!("Indent the right {b} the same as the left {b}."),
        IndentBase::AfterParen => {
            format!("Indent the right {b} the same as the first position after the preceding left parenthesis.")
        }
        IndentBase::ParentHashKey => format!("Indent the right {b} the same as the parent hash key."),
        IndentBase::StartOfLine => {
            format!("Indent the right {b} the same as the start of the line where the left {b} is.")
        }
    }
}

/// `check_right_bracket` / `check_right_brace`.
pub fn check_right(cx: &mut Cx, w: &Words, right: (usize, usize), base: (i64, IndentBase)) {
    let src = cx.src;
    if !crate::helpers::alignment::begins_its_line(src, right.0) {
        return;
    }
    let delta = base.0 - src.column(right.0) as i64;
    if delta == 0 {
        return;
    }
    let fixable = corrector_edits(src, right.0, right.1, delta, None);
    cx.add_with(right.0, right.1, right_message(w, &base.1), fixable);
}
