//! rubocop-sorbet's RBSAssertionCorrection mixin (the `AutocorrectToRBS` rewrite of `T.cast`,
//! `T.must`, `T.unsafe`, `T.bind`, `T.absurd` and `T.let` into `#: ...` comments) and
//! `RBSParser.rbs_annotation_after`, plus the small node predicates the S3 cops share.
//! `rbi_type_to_rbs` is `RBI::Type.parse_string(source).rbs_string` from `sorbet_rbi_type`.

use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use crate::source::Source;
use ruby_prism::{CallNode, Visit};

/// `(send (const nil? :T) :name ...)`: a send (not csend) of `name` on the bare constant `T`.
pub fn t_send<'pr>(node: &Node<'pr>, name: &str) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    (method_name(&call) == name
        && !is_csend(&call)
        && call.receiver().is_some_and(|r| r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"T")))
    .then_some(call)
}

/// `t_send` with exactly `nargs` parser arguments (a `&blk` counts), if given.
pub fn is_t_call(call: &CallNode, name: &str, nargs: Option<usize>) -> bool {
    t_send(&call.as_node(), name).is_some() && nargs.is_none_or(|n| send_args(call).len() == n)
}

/// A node that is a parser `send` rather than a `block`: no literal block attached.
pub fn no_literal_block(call: &CallNode) -> bool {
    call.block().is_none_or(|b| b.as_block_node().is_none())
}

/// Every node under `node` (itself included), in pre-order: `each_node` / `` `pattern ``.
pub fn descendants<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    struct V<'pr>(Vec<Node<'pr>>);
    impl<'pr> Visit<'pr> for V<'pr> {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            self.0.push(node);
        }
        fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
            self.0.push(node);
        }
    }
    let mut v = V(Vec::new());
    v.visit(node);
    v.0
}

/// `parent&.assignment? && parent.children.last.equal?(node)`: `parent` (a parser parent) assigns
/// the node at `range` (setter sends included: their last argument).
pub fn assigns(parent: Option<&Node>, range: (usize, usize)) -> bool {
    let Some(parent) = parent else { return false };
    let last = match parent.as_call_node() {
        Some(c) if c.equal_loc().is_some() => send_args(&c).pop(),
        Some(_) => None,
        None => assignment_parts(parent).map(|(_, v)| v),
    };
    last.is_some_and(|v| helpers::node_types::range(&v) == range)
}

/// The parser parent of the send `call` (whose Prism ancestors are `ancestors`): None when a
/// literal block owns it (the parent is that block).
pub fn send_parent<'a, 'pr>(call: &CallNode, ancestors: &'a [Node<'pr>]) -> Option<&'a Node<'pr>> {
    if !no_literal_block(call) {
        return None;
    }
    crate::helpers::parser_parent::parser_parent(ancestors)
}

/// A parser node's range: a call's `send` (without its literal block), else the node.
pub fn parser_range(node: &Node) -> (usize, usize) {
    match node.as_call_node() {
        Some(c) => send_range(&c),
        None => helpers::node_types::range(node),
    }
}

/// `RBSParser.rbs_annotation_after`: a `#:` comment with a non-blank annotation after the node on
/// its last line, separated from it by spaces and tabs only.
pub fn annotation_after(src: &Source, (start, end): (usize, usize)) -> bool {
    let _ = start;
    let line = src.line_of(end);
    let comment = src.comments.iter().filter(|c| src.line_of(c.start) == line).find(|c| {
        c.start >= end && src.text[end..c.start].bytes().all(|b| b == b' ' || b == b'\t')
    });
    comment.is_some_and(|c| {
        let text = &src.text[c.start..c.end];
        text.strip_prefix("#:").is_some_and(|rest| !helpers::ruby_string::strip(rest).is_empty())
    })
}

/// `assertion_ends_line?`: after the node and any spaces/tabs comes the end, a newline or `#`.
pub fn ends_line(src: &Source, (start, end): (usize, usize)) -> bool {
    let opts = Surrounding { side: Side::Right, newlines: false, ..Default::default() };
    let (_, e) = range_with_surrounding_space(src.text, start, end, opts);
    matches!(src.text.as_bytes().get(e), None | Some(b'\n' | b'\r' | b'#'))
}

/// `source_line.index(/\S/) == column`: the statement starts its line.
fn starts_line(src: &Source, (start, _): (usize, usize)) -> bool {
    let line = src.lines.get(src.line_of(start) - 1).copied().unwrap_or("");
    let first = line.chars().position(|c| !matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'));
    first == Some(src.column(start))
}

/// `assertion_statement(node, allow_assignment:)`: the node, or the assignment whose value it is
/// (only with `allow_assignment`). `parent` is the node's parser parent.
pub fn assertion_statement(parent: Option<&Node>, range: (usize, usize), allow_assignment: bool) -> Option<(usize, usize)> {
    if !assigns(parent, range) {
        return Some(range);
    }
    allow_assignment.then(|| parser_range(parent.unwrap()))
}

/// `rbs_assertion_autocorrectable?`, with the assertion statement already worked out.
pub fn autocorrectable(cx: &Cx, range: (usize, usize), statement: Option<(usize, usize)>) -> bool {
    cx.cfg.bool("AutocorrectToRBS").unwrap_or(false)
        && ends_line(cx.src, range)
        && !annotation_after(cx.src, range)
        && statement.is_some_and(|s| starts_line(cx.src, s))
}

/// `comments_within?(range)`.
pub fn comments_within(src: &Source, (start, end): (usize, usize)) -> bool {
    src.comments.iter().any(|c| c.start >= start && c.end <= end)
}

/// A parser `block`/`numblock` (not `itblock`) on one line, given its opening and closing.
fn single_line_block(src: &Source, params: Option<Node>, open: usize, close: usize) -> bool {
    !matches!(params, Some(Node::ItParametersNode { .. })) && src.line_of(open) == src.line_of(close)
}

/// `inside_single_line_block?`: a parser `block`/`numblock` ancestor on one line. A call's own
/// block is its parser parent, and so is the block of a call whose receiver or arguments hold it.
fn inside_single_line_block(src: &Source, call: &CallNode, ancestors: &[Node]) -> bool {
    let block_of = |n: &Node| {
        helpers::node_types::literal_block(n)
            .is_some_and(|b| single_line_block(src, b.parameters(), b.opening_loc().start_offset(), b.closing_loc().start_offset()))
    };
    if block_of(&call.as_node()) {
        return true;
    }
    ancestors.iter().enumerate().any(|(i, a)| match a {
        Node::BlockNode { .. } => {
            let b = a.as_block_node().unwrap();
            single_line_block(src, b.parameters(), b.opening_loc().start_offset(), b.closing_loc().start_offset())
        }
        Node::LambdaNode { .. } => {
            let l = a.as_lambda_node().unwrap();
            single_line_block(src, l.parameters(), l.opening_loc().start_offset(), l.closing_loc().start_offset())
        }
        _ => !ancestors.get(i + 1).is_some_and(|n| n.as_block_node().is_some()) && block_of(a),
    })
}

/// `one_matching_assertion?(container, node)`: one `send` of `name` on the constant `T` in the
/// container's send (its literal block excluded).
fn one_matching_assertion(container: &CallNode, name: &str) -> bool {
    let mut nodes: Vec<Node> = container.receiver().map(|r| descendants(&r)).unwrap_or_default();
    nodes.extend(send_args(container).iter().flat_map(descendants));
    nodes.push(container.as_node());
    nodes
        .iter()
        .filter(|n| {
            n.as_call_node().is_some_and(|c| {
                !is_csend(&c)
                    && method_name(&c) == name
                    && c.receiver().is_some_and(|r| helpers::const_name::const_name(&r).as_deref() == Some("T"))
            })
        })
        .count()
        == 1
}

/// The `:arguments` context of `call_argument_context`: the container call's send range, the
/// end of its `(` and start of its `)`, its argument items, the target item and whether it is a
/// (kw)splat.
struct ArgContext {
    send: (usize, usize),
    open_end: usize,
    close_start: usize,
    items: Vec<(usize, usize)>,
    target: usize,
    splat: bool,
}

fn call_argument_context(src: &Source, call: &CallNode, ancestors: &[Node]) -> Option<ArgContext> {
    if !no_literal_block(call) {
        return None;
    }
    let range = send_range(call);
    let pi = crate::helpers::parser_parent::parser_parent_index(ancestors, ancestors.len())?;
    let parent = &ancestors[pi];
    let up = |i: usize| crate::helpers::parser_parent::parser_parent_index(ancestors, i);
    let is_arg = |c: &CallNode, r: (usize, usize)| send_args(c).iter().any(|a| parser_range(a) == r);
    let (container, target, splat) = match parent {
        Node::CallNode { .. } if is_arg(&parent.as_call_node().unwrap(), range) => (pi, range, false),
        Node::SplatNode { .. } => {
            let ci = up(pi)?;
            let r = parser_range(parent);
            ancestors[ci].as_call_node().filter(|c| is_arg(c, r))?;
            (ci, r, true)
        }
        Node::AssocSplatNode { .. } | Node::AssocNode { .. } => {
            if let Some(pair) = parent.as_assoc_node()
                && parser_range(&pair.value()) != range
            {
                return None;
            }
            if parent.as_assoc_splat_node().is_some()
                && send_args(call).first().is_some_and(|a| a.as_keyword_hash_node().is_some())
            {
                return None;
            }
            let hi = up(pi)?;
            ancestors[hi].as_keyword_hash_node()?;
            let ci = up(hi)?;
            ancestors[ci].as_call_node().filter(|c| is_arg(c, helpers::node_types::range(&ancestors[hi])))?;
            (ci, helpers::node_types::range(parent), parent.as_assoc_splat_node().is_some())
        }
        _ => return None,
    };
    let c = ancestors[container].as_call_node()?;
    let send = send_range(&c);
    let (open, close) = (c.opening_loc()?, c.closing_loc()?);
    if open.as_slice() != b"(" || src.line_of(send.0) != src.line_of(send.1) || comments_within(src, send) {
        return None;
    }
    if !one_matching_assertion(&c, method_name(call)) {
        return None;
    }
    let items: Vec<(usize, usize)> = send_args(&c)
        .iter()
        .flat_map(|a| match a.as_keyword_hash_node() {
            Some(h) => h.elements().iter().map(|e| helpers::node_types::range(&e)).collect(),
            None => vec![parser_range(a)],
        })
        .collect();
    let target = items.iter().position(|&i| i == target)?;
    Some(ArgContext { send, open_end: open.end_offset(), close_start: close.start_offset(), items, target, splat })
}

/// `autocorrect_rbs_assertion(corrector, node, allow_assignment:) { [expression, annotation] }`:
/// the edits (none when the context does not allow the rewrite or `rewrite` fails).
pub fn rbs_assertion_edits(
    cx: &Cx,
    call: &CallNode,
    allow_assignment: bool,
    rewrite: impl FnOnce() -> Option<(String, String)>,
) -> Vec<(usize, usize, String)> {
    let src = cx.src;
    let range = send_range(call);
    let parent = send_parent(call, cx.ancestors);
    if autocorrectable(cx, range, assertion_statement(parent, range, allow_assignment)) {
        let Some((expression, annotation)) = rewrite() else { return Vec::new() };
        return vec![(range.0, range.1, format!("{expression} #: {annotation}"))];
    }
    // nested_rbs_assertion_autocorrectable?
    if !cx.cfg.bool("AutocorrectToRBS").unwrap_or(false)
        || src.line_of(range.0) != src.line_of(range.1)
        || inside_single_line_block(src, call, cx.ancestors)
        || annotation_after(src, range)
    {
        return Vec::new();
    }
    let Some(ctx) = call_argument_context(src, call, cx.ancestors) else { return Vec::new() };
    let Some((expression, annotation)) = rewrite() else { return Vec::new() };
    let line = src.lines.get(src.line_of(ctx.send.0) - 1).copied().unwrap_or("");
    let indentation: String = line.chars().take_while(|&c| c == ' ' || c == '\t').collect();
    let width = helpers::alignment::configured_indentation_width(cx).max(0) as usize;
    let item_indentation = format!("{indentation}{}", " ".repeat(width));
    let mut replacement = format!("{expression} #: {annotation}");
    if ctx.splat {
        let nested = format!("{item_indentation}{}", " ".repeat(width));
        replacement = format!("(\n{nested}{replacement}\n{item_indentation})");
    } else if ctx.target < ctx.items.len() - 1 {
        replacement = format!("{expression}, #: {annotation}");
    }
    let mut edits = vec![(range.0, range.1, replacement)];
    edits.push((ctx.open_end, ctx.items[0].0, format!("\n{item_indentation}")));
    for (i, w) in ctx.items.windows(2).enumerate() {
        let separator = if i == ctx.target && !ctx.splat { "" } else { "," };
        edits.push((w[0].1, w[1].0, format!("{separator}\n{item_indentation}")));
    }
    edits.push((ctx.items.last().unwrap().1, ctx.close_start, format!("\n{indentation}")));
    edits
}

/// `RBI::Type.parse_string(source).rbs_string`, None where it raises (sorbet_rbi_type).
pub fn rbi_type_to_rbs(source: &str) -> Option<String> {
    crate::helpers::sorbet_rbi_type::rbs_of(source).ok()
}

#[cfg(test)]
mod tests {
    use super::rbi_type_to_rbs as rbs;

    #[test]
    fn rbi_types() {
        assert_eq!(rbs("T::Array[String]").as_deref(), Some("Array[String]"));
        assert_eq!(rbs("T.any(String, Symbol)").as_deref(), Some("(String | Symbol)"));
        assert_eq!(rbs("T.nilable(T.proc.params(a: Integer).returns(String))").as_deref(), Some("(^(Integer a) -> String)?"));
        assert_eq!(rbs("{a: Integer, 'b c' => T::Boolean}").as_deref(), Some("{a: Integer, \"b c\" => bool}"));
        assert_eq!(rbs("T.class_of(Foo)[Bar]").as_deref(), Some("singleton(Foo)[Bar]"));
        assert_eq!(rbs("type"), None);
        assert_eq!(rbs("T.void"), None);
    }
}
