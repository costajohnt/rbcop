//! Helpers that let cops written against the parser gem's AST (what RuboCop uses) work on Prism's.
//!
//! Main shape differences to keep in mind when porting a cop:
//! - parser `send`/`csend` = Prism CallNode (csend when the call operator is `&.`).
//! - parser `(block (send ...) args body)` wraps the call; in Prism the BlockNode is the call's child.
//! - parser has no node for a single-statement body; Prism always has StatementsNode
//!   (several statements are a `begin` in parser). ProgramNode, ElseNode, ArgumentsNode have no
//!   parser counterpart either.

use crate::node_kind as kind;
use ruby_prism::{CallNode, Location, Node};

/// Source text of a location.
pub fn text<'pr>(loc: &Location<'pr>) -> &'pr str {
    std::str::from_utf8(loc.as_slice()).unwrap_or("")
}

/// Source text of a node.
pub fn node_text<'pr>(node: &Node<'pr>) -> &'pr str {
    let loc = node.location();
    std::str::from_utf8(loc.as_slice()).unwrap_or("")
}

pub fn method_name<'pr>(call: &CallNode<'pr>) -> &'pr str {
    std::str::from_utf8(call.name().as_slice()).unwrap_or("")
}

/// Positional arguments of a call (parser `send` children after the method name).
pub fn call_args<'pr>(call: &CallNode<'pr>) -> Vec<Node<'pr>> {
    call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default()
}

/// Byte range of the parser-gem `send` node for a call: like the CallNode's location, but without
/// an attached `{ }`/`do end` block (which parser wraps around the send). `&blk` stays included.
pub fn send_range(call: &CallNode) -> (usize, usize) {
    let loc = call.location();
    let Some(block) = call.block().filter(|b| b.as_block_node().is_some()) else {
        return (loc.start_offset(), loc.end_offset());
    };
    let end = [
        call.closing_loc().map(|l| l.end_offset()),
        call.arguments().map(|a| a.location().end_offset()),
        call.message_loc().map(|l| l.end_offset()),
        call.receiver().map(|r| r.location().end_offset()),
    ]
    .into_iter()
    .flatten()
    .filter(|&e| e <= block.location().start_offset())
    .max()
    .unwrap_or(loc.end_offset());
    (loc.start_offset(), end)
}

/// `foo&.bar` (parser `csend`).
pub fn is_csend(call: &CallNode) -> bool {
    call.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.")
}

/// A Prism ancestor classified by what the parser gem would make of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// send / csend
    Call,
    /// block / numblock / itblock / lambda literal
    Block,
    /// explicit `begin ... end`
    KwBegin,
    /// array / hash / range literal (rubocop-ast `literal?` parents that can hold a call)
    Literal,
    /// hash pair
    Pair,
    /// Prism-only wrapper with no parser counterpart
    Transparent,
    Other,
}

pub fn frame(node: &Node) -> Frame {
    match kind::of(node) {
        kind::CALL_NODE => Frame::Call,
        kind::BLOCK_NODE | kind::LAMBDA_NODE => Frame::Block,
        kind::BEGIN_NODE if node.as_begin_node().unwrap().begin_keyword_loc().is_some() => Frame::KwBegin,
        kind::ARRAY_NODE | kind::HASH_NODE | kind::KEYWORD_HASH_NODE | kind::RANGE_NODE => Frame::Literal,
        kind::ASSOC_NODE => Frame::Pair,
        kind::PROGRAM_NODE | kind::ELSE_NODE | kind::ARGUMENTS_NODE => Frame::Transparent,
        kind::STATEMENTS_NODE if node.as_statements_node().unwrap().body().iter().count() == 1 => Frame::Transparent,
        _ => Frame::Other,
    }
}

/// Parser-gem view of `ancestors` (outermost first, as in `Cx::ancestors`), nearest first.
pub fn parser_ancestors(ancestors: &[Node]) -> Vec<Frame> {
    (0..ancestors.len())
        .rev()
        .filter_map(|i| {
            let f = frame(&ancestors[i]);
            // `foo { x }` is (block (send foo) x) in parser: the call is not x's ancestor.
            let call_owning_block = f == Frame::Call && ancestors.get(i + 1).is_some_and(|n| frame(n) == Frame::Block);
            (f != Frame::Transparent && !call_owning_block).then_some(f)
        })
        .collect()
}

/// `in pat if cond` / `in pat unless cond`: Prism makes the guard an IfNode/UnlessNode wrapping the
/// pattern, parser an `if_guard`/`unless_guard`, so RuboCop's `on_if` never sees it.
pub fn is_pattern_guard(node: &Node, ancestors: &[Node]) -> bool {
    let loc = node.location();
    ancestors.last().and_then(|p| p.as_in_node()).is_some_and(|i| {
        let p = i.pattern().location();
        (p.start_offset(), p.end_offset()) == (loc.start_offset(), loc.end_offset())
    })
}

/// Parser `(begin ...)` unwrapping: `condition = condition.children.last while condition.begin_type?`.
/// None for an empty `()` (parser `(begin)`).
pub fn strip_parens<'pr>(node: Node<'pr>) -> Option<Node<'pr>> {
    let mut node = node;
    while let Some(p) = node.as_parentheses_node() {
        let body = p.body()?;
        node = match body.as_statements_node() {
            Some(s) => s.body().iter().last()?,
            None => body,
        };
    }
    Some(node)
}
