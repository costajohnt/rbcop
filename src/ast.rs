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

/// rubocop-ast ENUMERATOR_METHODS.
#[rustfmt::skip]
const ENUMERATOR_METHODS: &[&str] = &[
    "collect", "collect_concat", "detect", "downto", "each", "find", "find_all", "find_index", "inject", "loop", "map!",
    "map", "reduce", "reject", "reject!", "reverse_each", "select", "select!", "times", "upto",
];

/// rubocop-ast `enumerator_method?`: ENUMERATOR_METHODS or any `each_*`.
pub fn is_enumerator_method(name: &str) -> bool {
    ENUMERATOR_METHODS.contains(&name) || name.starts_with("each_")
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

/// Parser-gem-shaped ancestors with their parser source ranges, nearest first: indexes into
/// `ancestors` plus (start, end). Prism-only wrappers are skipped, a call that owns the block we
/// are inside is skipped, and a BlockNode gets the range parser gives the block (call start to block end).
pub fn parser_ancestor_ranges(ancestors: &[Node]) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for i in (0..ancestors.len()).rev() {
        let n = &ancestors[i];
        let f = frame(n);
        let call_owning_block =
            f == Frame::Call && ancestors.get(i + 1).is_some_and(|b| kind::of(b) == kind::BLOCK_NODE);
        if f == Frame::Transparent || call_owning_block {
            continue;
        }
        let loc = n.location();
        let start = if kind::of(n) == kind::BLOCK_NODE && i > 0 && kind::of(&ancestors[i - 1]) == kind::CALL_NODE {
            ancestors[i - 1].location().start_offset()
        } else {
            loc.start_offset()
        };
        out.push((i, start, loc.end_offset()));
    }
    out
}

/// A heredoc string literal (`<<~X`, `<<-X`, `<<X`), as rubocop-ast `heredoc?`.
pub fn is_heredoc(node: &Node) -> bool {
    let opening = match kind::of(node) {
        kind::STRING_NODE => node.as_string_node().unwrap().opening_loc(),
        kind::INTERPOLATED_STRING_NODE => node.as_interpolated_string_node().unwrap().opening_loc(),
        kind::X_STRING_NODE => Some(node.as_x_string_node().unwrap().opening_loc()),
        kind::INTERPOLATED_X_STRING_NODE => Some(node.as_interpolated_x_string_node().unwrap().opening_loc()),
        _ => None,
    };
    opening.is_some_and(|o| o.as_slice().starts_with(b"<<"))
}

/// Parser-gem `send` arguments: positional/keyword arguments plus a trailing `&blk` (block_pass).
pub fn send_args<'pr>(call: &CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut args = call_args(call);
    if let Some(b) = call.block().filter(|b| kind::of(b) == kind::BLOCK_ARGUMENT_NODE) {
        args.push(b);
    }
    args
}

/// Parser-gem source range of a def/block/lambda body. Prism wraps a body with `rescue`/`ensure`
/// in an implicit BeginNode spanning from the `do`/`def` to `end`; parser's `rescue`/`ensure`
/// node spans the body statements and clauses only.
pub fn body_range(body: &Node) -> (usize, usize) {
    let loc = body.location();
    let Some(b) = body.as_begin_node().filter(|b| b.begin_keyword_loc().is_none()) else {
        return (loc.start_offset(), loc.end_offset());
    };
    let start = [
        b.statements().map(|s| s.location().start_offset()),
        b.rescue_clause().map(|r| r.location().start_offset()),
        b.else_clause().map(|e| e.location().start_offset()),
        b.ensure_clause().map(|e| e.location().start_offset()),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(loc.start_offset());
    let end = if let Some(e) = b.ensure_clause() {
        e.statements().map_or(e.ensure_keyword_loc().end_offset(), |s| s.location().end_offset())
    } else if let Some(e) = b.else_clause() {
        e.statements().map_or(e.else_keyword_loc().end_offset(), |s| s.location().end_offset())
    } else if let Some(mut r) = b.rescue_clause() {
        while let Some(next) = r.subsequent() {
            r = next;
        }
        resbody_end(&b, &r)
    } else {
        b.statements().map_or(loc.end_offset(), |s| s.location().end_offset())
    };
    (start, end)
}

/// End of the parser `resbody` of the last rescue clause `r` of `b`: `helpers::resbody::range`
/// without a Source (the BeginNode's own source covers the gap after the clause head).
fn resbody_end(b: &ruby_prism::BeginNode, r: &ruby_prism::RescueNode) -> usize {
    if let Some(s) = r.statements() {
        return s.location().end_offset();
    }
    // Prism's clause ends at its head (keyword, exceptions or `=> var`; not `then`).
    let head_end = r.location().end_offset();
    let (b_start, b_src) = (b.location().start_offset(), b.location().as_slice());
    crate::helpers::resbody::semicolon_end(head_end, b_src.get(head_end.saturating_sub(b_start)..).unwrap_or_default())
}

/// `.rest()` of a MultiWriteNode / MultiTargetNode / ParametersNode as parser sees it: Prism's
/// ImplicitRestNode (trailing comma: `a, = x`, `|a, b,|`) has no parser node.
pub fn parser_rest<'pr>(rest: Option<Node<'pr>>) -> Option<Node<'pr>> {
    rest.filter(|r| r.as_implicit_rest_node().is_none())
}

/// Parser-gem `args` children of a parameter list, in source order.
pub fn parameter_nodes<'pr>(params: &ruby_prism::ParametersNode<'pr>) -> Vec<Node<'pr>> {
    let mut out: Vec<Node> = params.requireds().iter().chain(params.optionals().iter()).collect();
    out.extend(parser_rest(params.rest()));
    out.extend(params.posts().iter());
    out.extend(params.keywords().iter());
    out.extend(params.keyword_rest());
    out.extend(params.block().map(|b| b.as_node()));
    out.sort_by_key(|n| n.location().start_offset());
    out
}

/// rubocop-ast `assignment?` (lvasgn ivasgn cvasgn gvasgn casgn masgn op_asgn or_asgn and_asgn):
/// the operator location and the assigned value. Setter sends (`a.b = c`) are not included.
pub fn assignment_parts<'pr>(node: &Node<'pr>) -> Option<(Location<'pr>, Node<'pr>)> {
    macro_rules! parts {
        ($($as:ident . $op:ident),* $(,)?) => {
            $(if let Some(n) = node.$as() {
                return Some((n.$op(), n.value()));
            })*
        };
    }
    parts!(
        as_local_variable_write_node.operator_loc,
        as_instance_variable_write_node.operator_loc,
        as_class_variable_write_node.operator_loc,
        as_global_variable_write_node.operator_loc,
        as_constant_write_node.operator_loc,
        as_constant_path_write_node.operator_loc,
        as_multi_write_node.operator_loc,
        as_local_variable_operator_write_node.binary_operator_loc,
        as_instance_variable_operator_write_node.binary_operator_loc,
        as_class_variable_operator_write_node.binary_operator_loc,
        as_global_variable_operator_write_node.binary_operator_loc,
        as_constant_operator_write_node.binary_operator_loc,
        as_constant_path_operator_write_node.binary_operator_loc,
        as_call_operator_write_node.binary_operator_loc,
        as_index_operator_write_node.binary_operator_loc,
        as_local_variable_or_write_node.operator_loc,
        as_instance_variable_or_write_node.operator_loc,
        as_class_variable_or_write_node.operator_loc,
        as_global_variable_or_write_node.operator_loc,
        as_constant_or_write_node.operator_loc,
        as_constant_path_or_write_node.operator_loc,
        as_call_or_write_node.operator_loc,
        as_index_or_write_node.operator_loc,
        as_local_variable_and_write_node.operator_loc,
        as_instance_variable_and_write_node.operator_loc,
        as_class_variable_and_write_node.operator_loc,
        as_global_variable_and_write_node.operator_loc,
        as_constant_and_write_node.operator_loc,
        as_constant_path_and_write_node.operator_loc,
        as_call_and_write_node.operator_loc,
        as_index_and_write_node.operator_loc,
    );
    None
}

/// Node kinds `assignment_parts` recognises.
pub const ASSIGNMENT_KINDS: &[usize] = &[
    kind::LOCAL_VARIABLE_WRITE_NODE,
    kind::INSTANCE_VARIABLE_WRITE_NODE,
    kind::CLASS_VARIABLE_WRITE_NODE,
    kind::GLOBAL_VARIABLE_WRITE_NODE,
    kind::CONSTANT_WRITE_NODE,
    kind::CONSTANT_PATH_WRITE_NODE,
    kind::MULTI_WRITE_NODE,
    kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
    kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::CONSTANT_OPERATOR_WRITE_NODE,
    kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
    kind::CALL_OPERATOR_WRITE_NODE,
    kind::INDEX_OPERATOR_WRITE_NODE,
    kind::LOCAL_VARIABLE_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_OR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
    kind::CONSTANT_OR_WRITE_NODE,
    kind::CONSTANT_PATH_OR_WRITE_NODE,
    kind::CALL_OR_WRITE_NODE,
    kind::INDEX_OR_WRITE_NODE,
    kind::LOCAL_VARIABLE_AND_WRITE_NODE,
    kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
    kind::CLASS_VARIABLE_AND_WRITE_NODE,
    kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
    kind::CONSTANT_AND_WRITE_NODE,
    kind::CONSTANT_PATH_AND_WRITE_NODE,
    kind::CALL_AND_WRITE_NODE,
    kind::INDEX_AND_WRITE_NODE,
];

/// rubocop-ast `assignment?` on any node: the assignment types, plus setter sends (on send and
/// csend nodes `assignment?` is an alias of `setter_method?`).
pub fn is_assignment(node: &Node) -> bool {
    node.as_call_node().map_or_else(|| assignment_parts(node).is_some(), |c| c.equal_loc().is_some())
}

/// Parser `dstr`: an interpolated string, or a string literal spanning several lines (parser splits
/// it into one `str` per line; a heredoc with more than one body line).
pub fn is_parser_dstr(node: &Node) -> bool {
    if node.as_interpolated_string_node().is_some() {
        return true;
    }
    let Some(s) = node.as_string_node() else { return false };
    // An empty percent literal or heredoc has no string token: parser builds an empty `dstr`.
    if s.content_loc().start_offset() == s.content_loc().end_offset()
        && s.opening_loc().is_some_and(|o| matches!(o.as_slice().first(), Some(b'%' | b'<')))
    {
        return true;
    }
    if is_heredoc(node) {
        return text(&s.content_loc()).trim_end_matches('\n').contains('\n');
    }
    // Parser splits a literal only on a newline before the last character of its content, so a
    // literal whose only newline ends the content (`"a<newline>"`) stays a `str`. In an
    // interpolating literal a backslash-newline is a line continuation (not in the value), so it
    // does not split either; in a single-quoted one it stays in the value and does.
    let content = text(&s.content_loc());
    let body = content.strip_suffix('\n').unwrap_or(content).as_bytes();
    let opening = s.opening_loc().map(|o| text(&o)).unwrap_or("");
    let interpolating = !(opening.starts_with('\'') || opening.starts_with("%q"));
    body.iter().enumerate().any(|(i, &b)| {
        b == b'\n' && !(interpolating && body[..i].iter().rev().take_while(|&&c| c == b'\\').count() % 2 == 1)
    })
}

/// Parser `dsym`: an interpolated symbol, or a symbol literal spanning several lines (split like
/// `is_parser_dstr`).
pub fn is_parser_dsym(node: &Node) -> bool {
    if node.as_interpolated_symbol_node().is_some() {
        return true;
    }
    let Some(sym) = node.as_symbol_node() else { return false };
    let content = sym.value_loc().map_or("", |v| text(&v));
    // `:""`, `:''` and `%s()` are empty `dsym`s (an empty label `"":` stays a `sym`).
    if content.is_empty()
        && sym.opening_loc().is_some_and(|o| o.as_slice().starts_with(b":") || o.as_slice().starts_with(b"%s"))
    {
        return true;
    }
    let body = content.strip_suffix('\n').unwrap_or(content).as_bytes();
    let opening = sym.opening_loc().map(|o| text(&o)).unwrap_or("");
    let interpolating = !(opening.contains('\'') || opening.starts_with("%s"));
    body.iter().enumerate().any(|(i, &b)| {
        b == b'\n' && !(interpolating && body[..i].iter().rev().take_while(|&&c| c == b'\\').count() % 2 == 1)
    })
}

/// Value of an integer literal node (parser `int`; Prism keeps a leading `-` in the literal), or None
/// for other nodes and values beyond i128.
pub fn int_literal_value(node: &Node) -> Option<i128> {
    let value = node.as_integer_node()?.value();
    let (neg, digits) = value.to_u32_digits();
    let mut v: i128 = 0;
    for d in digits.iter().rev() {
        v = v.checked_mul(1 << 32)?.checked_add(*d as i128)?;
    }
    Some(if neg { -v } else { v })
}

/// Whether the tree under `node` is nested more than `max` levels deep. The probe stops descending
/// once it passes `max`, so it never recurses deeper than that itself.
pub fn deeper_than(node: &Node, max: usize) -> bool {
    use ruby_prism::Visit;
    struct Probe {
        depth: usize,
        max: usize,
    }
    struct TooDeep;
    impl<'pr> Visit<'pr> for Probe {
        fn visit_branch_node_enter(&mut self, _: Node<'pr>) {
            self.depth += 1;
            if self.depth > self.max {
                // resume_unwind skips the panic hook: this is control flow, not an error.
                std::panic::resume_unwind(Box::new(TooDeep));
            }
        }
        fn visit_branch_node_leave(&mut self) {
            self.depth -= 1;
        }
    }
    let mut probe = Probe { depth: 0, max };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| probe.visit(node))).is_err()
}
