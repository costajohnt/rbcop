//! rubocop-sorbet's RBSAssertionCorrection mixin (the `AutocorrectToRBS` rewrite of `T.cast`,
//! `T.must`, `T.unsafe`, `T.bind`, `T.absurd` and `T.let` into `#: ...` comments) and
//! `RBSParser.rbs_annotation_after`, plus the small node predicates the S3 cops share.
//!
//! `rbi_type_to_rbs` is a minimal port of `RBI::Type.parse_string(source).rbs_string` (rbi 0.5.0
//! type_parser.rb and rbs_printer.rb's TypePrinter). ponytail: it lives here until the S4 helper
//! `sorbet_rbi_type` exists; S4 can move it there.

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

/// A parsed RBI type as the TypePrinter needs it: its RBS text, and whether it is a
/// `Type::Simple` (what `T.class_of` takes) or a `Type::Proc` (parenthesized when nilable).
struct Ty {
    rbs: String,
    simple: bool,
    proc: bool,
}

impl Ty {
    fn new(rbs: String) -> Self {
        Ty { rbs, simple: false, proc: false }
    }
}

/// TypePrinter#translate_t_type, on a name with whitespace removed.
fn simple(name: &str) -> Ty {
    let name: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let rbs = match name.as_str() {
        "T::Array" => "Array",
        "T::Enumerable" => "Enumerable",
        "T::Enumerator" => "Enumerator",
        "T::Enumerator::Chain" => "Enumerator::Chain",
        "T::Enumerator::Lazy" => "Enumerator::Lazy",
        "T::Hash" => "Hash",
        "T::Set" => "Set",
        "T::Range" => "Range",
        n => n,
    };
    Ty { rbs: rbs.to_string(), simple: true, proc: false }
}

/// `RBI::Type.parse_string(source).rbs_string`, None where it raises `RBI::Type::Error`.
pub fn rbi_type_to_rbs(source: &str) -> Option<String> {
    let result = ruby_prism::parse(source.as_bytes());
    if result.errors().next().is_some() {
        return None;
    }
    let program = result.node();
    let stmts: Vec<Node> = program.as_program_node()?.statements().body().iter().collect();
    let [node] = stmts.as_slice() else { return None };
    Some(parse_node(node)?.rbs)
}

/// `RBI::Type.t?`: `T` or `::T`.
fn is_t(node: Option<Node>) -> bool {
    match node {
        Some(Node::ConstantReadNode { .. }) => node.unwrap().as_constant_read_node().unwrap().name().as_slice() == b"T",
        Some(Node::ConstantPathNode { .. }) => {
            let p = node.unwrap().as_constant_path_node().unwrap();
            p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"T")
        }
        _ => false,
    }
}

fn args<'pr>(call: &CallNode<'pr>) -> Vec<Node<'pr>> {
    call_args(call)
}

fn list(nodes: &[Node], sep: &str) -> Option<String> {
    let parts: Option<Vec<String>> = nodes.iter().map(|n| parse_node(n).map(|t| t.rbs)).collect();
    Some(parts?.join(sep))
}

fn parse_node(node: &Node) -> Option<Ty> {
    match node {
        Node::ConstantReadNode { .. } => Some(simple(node_text(node))),
        Node::ConstantPathNode { .. } => {
            let p = node.as_constant_path_node().unwrap();
            if is_t(p.parent()) && p.name().is_some_and(|n| n.as_slice() == b"Boolean") {
                Some(Ty::new("bool".into()))
            } else {
                Some(simple(node_text(node)))
            }
        }
        // parse_constant_assignment: a `T.type_alias` value makes a TypeAlias, which the
        // TypePrinter cannot print (RuboCop raises past the cop's rescue): treated as an error.
        Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. } => {
            let value = match node {
                Node::ConstantWriteNode { .. } => node.as_constant_write_node().unwrap().value(),
                _ => node.as_constant_path_write_node().unwrap().value(),
            };
            let alias = value.as_call_node().is_some_and(|c| is_t(c.receiver()) && method_name(&c) == "type_alias");
            (!alias).then(|| simple(node_text(node)))
        }
        Node::CallNode { .. } => parse_call(&node.as_call_node().unwrap()),
        Node::ArrayNode { .. } => {
            let elements: Vec<Node> = node.as_array_node().unwrap().elements().iter().collect();
            Some(Ty::new(format!("[{}]", list(&elements, ", ")?)))
        }
        Node::HashNode { .. } => parse_shape(node.as_hash_node().unwrap().elements().iter().collect()),
        Node::KeywordHashNode { .. } => parse_shape(node.as_keyword_hash_node().unwrap().elements().iter().collect()),
        Node::ParenthesesNode { .. } => {
            let body = node.as_parentheses_node().unwrap().body()?;
            let children: Vec<Node> = body.as_statements_node()?.body().iter().collect();
            let [child] = children.as_slice() else { return None };
            parse_node(child)
        }
        _ => None,
    }
}

fn parse_shape(elements: Vec<Node>) -> Option<Ty> {
    // `.to_h`: a repeated key keeps its first position and its last value.
    let mut entries: Vec<(String, String)> = Vec::new();
    for e in elements {
        let assoc = e.as_assoc_node()?;
        let key = assoc.key();
        let prefix = if let Some(sym) = key.as_symbol_node() {
            let k = String::from_utf8_lossy(sym.unescaped()).into_owned();
            static IDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[a-zA-Z_]+[a-zA-Z0-9_]*\z").unwrap());
            if IDENT.is_match(&k) { format!("{k}: ") } else { format!("\"{k}\": ") }
        } else {
            format!("\"{}\" => ", text(&key.as_string_node()?.content_loc()))
        };
        let value = parse_node(&assoc.value())?.rbs;
        match entries.iter_mut().find(|(k, _)| *k == prefix) {
            Some(entry) => entry.1 = value,
            None => entries.push((prefix, value)),
        }
    }
    let body: Vec<String> = entries.into_iter().map(|(k, v)| format!("{k}{v}")).collect();
    Some(Ty::new(format!("{{{}}}", body.join(", "))))
}

fn parse_call(node: &CallNode) -> Option<Ty> {
    let recv = node.receiver();
    let name = method_name(node);
    let a = args(node);
    if name == "void" {
        if !a.is_empty() {
            return None;
        }
        if recv.is_none() {
            return Some(Ty::new("void".into()));
        }
    }
    if name == "[]" {
        let recv = recv?;
        match &recv {
            Node::ConstantReadNode { .. } => {
                if a.is_empty() {
                    return None;
                }
                return Some(Ty::new(format!("{}[{}]", simple(node_text(&recv)).rbs, list(&a, ", ")?)));
            }
            Node::ConstantPathNode { .. } => {
                let p = recv.as_constant_path_node().unwrap();
                let last = p.name().map(|n| n.as_slice());
                let wrapper = if is_t(p.parent()) && last == Some(b"Class") {
                    Some("Class")
                } else if is_t(p.parent()) && last == Some(b"Module") {
                    Some("Module")
                } else {
                    None
                };
                if let Some(w) = wrapper {
                    let [arg] = a.as_slice() else { return None };
                    return Some(Ty::new(format!("{w}[{}]", parse_node(arg)?.rbs)));
                }
                if a.is_empty() {
                    return None;
                }
                return Some(Ty::new(format!("{}[{}]", simple(node_text(&recv)).rbs, list(&a, ", ")?)));
            }
            Node::CallNode { .. } => {
                let c = recv.as_call_node().unwrap();
                if is_t(c.receiver()) && method_name(&c) == "class_of" {
                    let [ty] = args(&c).try_into().ok()?;
                    let ty = parse_node(&ty)?;
                    if !ty.simple || a.is_empty() {
                        return None;
                    }
                    return Some(Ty::new(format!("singleton({})[{}]", ty.rbs, list(&a, ", ")?)));
                }
                return None;
            }
            _ => return None,
        }
    }
    if let Some(t) = parse_proc(node) {
        return t;
    }
    if !is_t(recv) {
        return None;
    }
    let exactly = |n: usize| (a.len() == n).then_some(());
    let rbs = match name {
        "nilable" => {
            exactly(1)?;
            let inner = parse_node(&a[0])?;
            if inner.proc { format!("({})?", inner.rbs) } else { format!("{}?", inner.rbs) }
        }
        "anything" => exactly(0).map(|_| "top".to_string())?,
        "untyped" => exactly(0).map(|_| "untyped".to_string())?,
        "noreturn" => exactly(0).map(|_| "bot".to_string())?,
        "self_type" => exactly(0).map(|_| "self".to_string())?,
        "attached_class" => exactly(0).map(|_| "instance".to_string())?,
        "class_of" => {
            exactly(1)?;
            let ty = parse_node(&a[0])?;
            if !ty.simple {
                return None;
            }
            format!("singleton({})", ty.rbs)
        }
        "all" | "any" => {
            if a.len() < 2 {
                return None;
            }
            format!("({})", list(&a, if name == "all" { " & " } else { " | " })?)
        }
        "type_parameter" => {
            exactly(1)?;
            let sym = a[0].as_symbol_node()?;
            node_text(&sym.as_node()).strip_prefix(':').unwrap_or(node_text(&sym.as_node())).to_string()
        }
        _ => return None,
    };
    Some(Ty::new(rbs))
}

/// `T.proc...` (`t_proc?` then `parse_proc`): None when `node` is not one, Some(None) on an error.
fn parse_proc(node: &CallNode) -> Option<Option<Ty>> {
    // call_chain: the receivers down to the first non-call, outermost call last.
    let mut chain: Vec<Node> = vec![node.as_node()];
    let mut receiver = node.receiver();
    while let Some(r) = receiver {
        let next = r.as_call_node().and_then(|c| c.receiver());
        let is_call = r.as_call_node().is_some();
        chain.insert(0, r);
        if !is_call {
            break;
        }
        receiver = next;
    }
    if chain.len() < 2 || !is_t(Some(chain.remove(0))) {
        return None;
    }
    if !chain[0].as_call_node().is_some_and(|c| method_name(&c) == "proc") {
        return None;
    }
    let mut params: Vec<(String, String)> = Vec::new();
    let mut returns = "void".to_string();
    let mut bind: Option<String> = None;
    for call in chain.iter().rev() {
        let call = call.as_call_node().unwrap();
        let a = args(&call);
        match method_name(&call) {
            "params" => {
                let Some(hash) = a.first().and_then(|h| h.as_keyword_hash_node()) else { return Some(None) };
                params.clear();
                for e in hash.elements().iter() {
                    let Some(assoc) = e.as_assoc_node() else { return Some(None) };
                    let key = node_text(&assoc.key());
                    let key = key.strip_suffix(':').unwrap_or(key).to_string();
                    let Some(value) = parse_node(&assoc.value()) else { return Some(None) };
                    match params.iter_mut().find(|(k, _)| *k == key) {
                        Some(p) => p.1 = value.rbs,
                        None => params.push((key, value.rbs)),
                    }
                }
            }
            "returns" => {
                let ([arg], Some(t)) = (a.as_slice(), a.first().and_then(parse_node)) else { return Some(None) };
                let _ = arg;
                returns = t.rbs;
            }
            "void" => returns = "void".to_string(),
            "proc" => break,
            "bind" => {
                let ([_], Some(t)) = (a.as_slice(), a.first().and_then(parse_node)) else { return Some(None) };
                bind = Some(t.rbs);
            }
            _ => return Some(None),
        }
    }
    let mut out = "^".to_string();
    if !params.is_empty() {
        let ps: Vec<String> = params.iter().map(|(k, v)| format!("{v} {k}")).collect();
        out.push_str(&format!("({}) ", ps.join(", ")));
    }
    if let Some(b) = bind {
        out.push_str(&format!("[self: {b}] "));
    }
    out.push_str("-> ");
    out.push_str(&returns);
    Some(Some(Ty { rbs: out, simple: false, proc: true }))
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
