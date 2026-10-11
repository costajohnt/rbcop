use crate::helpers::documentation_comment::{comments_taken_by_ancestor, documentation_comment, enclosing_start};
use crate::helpers::comment_association::leading;
use crate::helpers::parser_parent::parser_parent_index;
use crate::prelude::*;
use crate::source::Source;
use ruby_prism::Visit;

pub const NAME: &str = "Style/Documentation";
pub static COP: Documentation = Documentation;
pub struct Documentation;

static NODOC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^#(?-u:\s)*:nodoc:").unwrap());
// `"\s+all\s*$"` is a double-quoted Ruby string there: its `\s` is a literal space.
static NODOC_ALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^#(?-u:\s)*:nodoc: +all *$").unwrap());

/// Every Prism node range in the file, sorted by end (for comment-association frontiers).
#[derive(Default)]
struct Ranges(Vec<(usize, usize)>);

impl<'pr> Visit<'pr> for Ranges {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.0.push((node.location().start_offset(), node.location().end_offset()));
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.0.push((node.location().start_offset(), node.location().end_offset()));
    }
}

/// The node ends and the node ranges, both sorted by end.
type NodeSpans = (Vec<usize>, Vec<(usize, usize)>);

#[derive(Default)]
struct State {
    ranges: Option<NodeSpans>,
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// Comment::Associator, approximated from node ranges: the comments of the node at `r` that a
/// parser-gem association would give it, in order (leading, interior, decorating).
fn associated<'a>(src: &'a Source, ranges: &[(usize, usize)], r: (usize, usize)) -> Vec<&'a Comment> {
    // Frontier: the latest end of a node finished before `r` starts.
    // Enclosing nodes took the comments before their own start.
    let idx = ranges.partition_point(|&(_, e)| e <= r.0);
    let prev = idx.checked_sub(1).map(|i| ranges[i].1).filter(|&e| e > 0);
    let enclosing = ranges.iter().filter(|&&(s, e)| s < r.0 && e >= r.1).map(|&(s, _)| s).max().unwrap_or(0);
    let after = prev.unwrap_or(0).max(enclosing);
    let mut out = leading(src, r.0, after, prev.map(|e| src.line_of(e - 1)));
    // Interior comments not taken by a descendant (before its end or decorating its last line).
    let last_child_end = ranges.iter().filter(|&&(s, e)| s >= r.0 && e <= r.1 && (s, e) != r && e > s).map(|&(_, e)| e).max();
    out.extend(src.comments.iter().filter(|c| {
        c.start >= r.0
            && c.end <= r.1
            && last_child_end.is_none_or(|e| c.start >= e && src.line_of(c.start) > src.line_of(e - 1))
    }));
    let last_line = src.line_of(r.1.saturating_sub(1).max(r.0));
    let child_on_last_line = ranges
        .iter()
        .any(|&(s, e)| s >= r.0 && e <= r.1 && (s, e) != r && e > s && src.line_of(e - 1) == last_line);
    // A node finished before `r` starts on that line already took the comment (the associator's
    // cursor gives a decorating comment to the first node whose trailing pass sees it).
    let taken_earlier = prev.is_some_and(|e| src.line_of(e - 1) == last_line);
    if !child_on_last_line && !taken_earlier {
        out.extend(src.comments.iter().filter(|c| c.start >= r.1 && src.line_of(c.start) == last_line));
    }
    out
}

/// The parser `children.first` of a parser ancestor, as a range: Ok(None) when it is not a node
/// with comments (a method name symbol), Err(()) when it is nil (the recursion stops).
fn first_child(anc: &[Node], j: usize) -> Result<Option<(usize, usize)>, ()> {
    let n = &anc[j];
    let first_stmt = |s: Option<ruby_prism::StatementsNode>| s.and_then(|s| s.body().iter().next()).map(|f| range(&f));
    let r = match n {
        Node::ClassNode { .. } => Some(range(&n.as_class_node().unwrap().constant_path())),
        Node::ModuleNode { .. } => Some(range(&n.as_module_node().unwrap().constant_path())),
        Node::SingletonClassNode { .. } => Some(range(&n.as_singleton_class_node().unwrap().expression())),
        Node::BlockNode { .. } if j > 0 && anc[j - 1].as_call_node().is_some() => {
            Some(send_range(&anc[j - 1].as_call_node().unwrap()))
        }
        Node::LambdaNode { .. } => {
            let op = n.as_lambda_node().unwrap().operator_loc();
            Some((op.start_offset(), op.end_offset()))
        }
        Node::StatementsNode { .. } => first_stmt(n.as_statements_node()),
        Node::BeginNode { .. } => first_stmt(n.as_begin_node().unwrap().statements()),
        Node::ParenthesesNode { .. } => n.as_parentheses_node().unwrap().body().and_then(|b| match b.as_statements_node() {
            Some(s) => first_stmt(Some(s)),
            None => Some(range(&b)),
        }),
        Node::DefNode { .. } => return Ok(None),
        Node::IfNode { .. } => Some(range(&n.as_if_node().unwrap().predicate())),
        Node::UnlessNode { .. } => Some(range(&n.as_unless_node().unwrap().predicate())),
        Node::WhileNode { .. } => Some(range(&n.as_while_node().unwrap().predicate())),
        Node::UntilNode { .. } => Some(range(&n.as_until_node().unwrap().predicate())),
        Node::CaseNode { .. } => n.as_case_node().unwrap().predicate().map(|p| range(&p)),
        Node::WhenNode { .. } => n.as_when_node().unwrap().conditions().iter().next().map(|c| range(&c)),
        Node::CallNode { .. } => n.as_call_node().unwrap().receiver().map(|r| range(&r)),
        // ponytail: other parents (rescue/ensure clauses, assignments, ...) end the search.
        _ => None,
    };
    r.map(Some).ok_or(())
}

/// Documentation#nodoc_comment? for a node whose `children.first` is `first` and whose first
/// line starts at `start`.
fn nodoc_here(src: &Source, ranges: &[(usize, usize)], first: (usize, usize), start: usize, all: bool) -> bool {
    associated(src, ranges, first).first().is_some_and(|c| {
        let text = src.slice(c.start, c.end);
        src.line_of(c.start) == src.line_of(start) && if all { NODOC_ALL.is_match(text) } else { NODOC.is_match(text) }
    })
}

/// The recursion of nodoc_comment? over the parser ancestors from `anc[..i]` upwards.
fn nodoc_up(src: &Source, ranges: &[(usize, usize)], anc: &[Node], mut i: usize) -> bool {
    while let Some(j) = parser_parent_index(anc, i) {
        match first_child(anc, j) {
            Err(()) => return false,
            Ok(Some(first)) => {
                let start = if kind::of(&anc[j]) == kind::BLOCK_NODE { anc[j - 1].location().start_offset() } else { anc[j].location().start_offset() };
                if nodoc_here(src, ranges, first, start, true) {
                    return true;
                }
            }
            Ok(None) => {}
        }
        i = j;
    }
    false
}

/// `(const (const nil? _) _)` search target: the first such ConstantPathNode in pre-order.
#[derive(Default)]
struct OuterModule(Option<(usize, usize, usize, usize)>);

impl<'pr> Visit<'pr> for OuterModule {
    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode<'pr>) {
        if self.0.is_none()
            && let Some(p) = node.parent()
            && p.as_constant_read_node().is_some()
        {
            let (s, e) = range(&node.as_node());
            self.0 = Some((s, e, p.location().start_offset(), p.location().end_offset()));
            return;
        }
        ruby_prism::visit_constant_path_node(self, node);
    }
}

/// rubocop-ast children of a parser node are all nodes satisfying include_statement_only?.
fn include_statement_only(node: &Node) -> bool {
    if let Some(c) = node.as_call_node()
        && c.receiver().is_none()
        && matches!(method_name(&c), "include" | "extend" | "prepend")
        && c.block().is_none()
        && matches!(call_args(&c).as_slice(), [a] if a.as_constant_read_node().is_some() || a.as_constant_path_node().is_some())
    {
        return true;
    }
    // ponytail: childless leaves and node-only containers; other parser children are symbols,
    // strings or nil, which fail.
    match node {
        Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::NilNode { .. }
        | Node::SelfNode { .. }
        | Node::ForwardingSuperNode { .. }
        | Node::RetryNode { .. }
        | Node::RedoNode { .. } => true,
        // (return args...): bare `return` (valid in a class body below 2.5) has no children.
        Node::ReturnNode { .. } => node
            .as_return_node()
            .unwrap()
            .arguments()
            .is_none_or(|a| a.arguments().iter().all(|n| include_statement_only(&n))),
        Node::StatementsNode { .. } => node.as_statements_node().unwrap().body().iter().all(|n| include_statement_only(&n)),
        Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().iter().all(|n| include_statement_only(&n)),
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().next().is_none(),
        // `//`: (regexp (regopt)), no parts and no option symbols
        Node::RegularExpressionNode { .. } => {
            let r = node.as_regular_expression_node().unwrap();
            r.content_loc().as_slice().is_empty() && r.closing_loc().as_slice().len() == 1
        }
        // an empty `dstr` (`%Q()`, empty heredoc) has no children
        Node::StringNode { .. } => is_parser_dstr(node) && node.as_string_node().unwrap().unescaped().is_empty(),
        Node::ParenthesesNode { .. } => node.as_parentheses_node().unwrap().body().is_some_and(|b| include_statement_only(&b)),
        Node::AndNode { .. } => {
            let a = node.as_and_node().unwrap();
            include_statement_only(&a.left()) && include_statement_only(&a.right())
        }
        Node::OrNode { .. } => {
            let o = node.as_or_node().unwrap();
            include_statement_only(&o.left()) && include_statement_only(&o.right())
        }
        // (sclass expr body): a missing body is nil, which fails.
        Node::SingletonClassNode { .. } => {
            let c = node.as_singleton_class_node().unwrap();
            include_statement_only(&c.expression()) && c.body().is_some_and(|b| include_statement_only(&b))
        }
        _ => false,
    }
}

fn constant_definition(node: &Node) -> bool {
    matches!(
        node,
        Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. }
    )
}

/// `(send nil? {:public_constant :private_constant} ({sym str} _))`
fn constant_visibility_declaration(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        c.receiver().is_none()
            && matches!(method_name(&c), "public_constant" | "private_constant")
            && c.block().is_none()
            && matches!(call_args(&c).as_slice(), [a] if a.as_symbol_node().is_some() || (a.as_string_node().is_some() && !is_parser_dstr(a)))
    })
}

/// Documentation#namespace?
fn namespace(body: &Option<Node>) -> bool {
    let Some(body) = body.as_ref() else { return false };
    fn declarations<'pr>(mut children: impl Iterator<Item = Node<'pr>>) -> bool {
        children.all(|c| constant_definition(&c) || constant_visibility_declaration(&c))
    }
    match body.as_statements_node() {
        Some(s) if s.body().iter().count() > 1 => declarations(s.body().iter()),
        Some(s) => s.body().iter().next().is_some_and(|c| match c.as_parentheses_node() {
            // A parenthesized body is a parser `begin` too: its statements (none for `()`).
            Some(p) => match p.body() {
                Some(b) => match b.as_statements_node() {
                    Some(inner) => declarations(inner.body().iter()),
                    None => declarations(std::iter::once(b)),
                },
                None => true,
            },
            None => constant_definition(&c),
        }),
        None => false,
    }
}

/// Documentation#qualify_const, flattened.
fn qualify_const(src: &Source, node: &Node, out: &mut Vec<String>) {
    match node {
        Node::ConstantReadNode { .. } => out.push(String::from_utf8_lossy(node.as_constant_read_node().unwrap().name().as_slice()).into_owned()),
        Node::ConstantPathNode { .. } => {
            let p = node.as_constant_path_node().unwrap();
            match p.parent() {
                Some(parent) => qualify_const(src, &parent, out),
                None => out.push("::".to_string()),
            }
            if let Some(n) = p.name() {
                out.push(String::from_utf8_lossy(n.as_slice()).into_owned());
            }
        }
        _ => out.push(src.slice(range(node).0, range(node).1).to_string()),
    }
}

impl Cop for Documentation {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (is_class, name, body) = match node.as_class_node() {
            Some(c) => (true, c.constant_path(), c.body()),
            None => {
                let m = node.as_module_node().unwrap();
                (false, m.constant_path(), m.body())
            }
        };
        if is_class && body.is_none() {
            return;
        }
        if namespace(&body) {
            return;
        }
        let anc = cx.ancestors;
        if cx.state::<State>().ranges.is_none() {
            let mut v = Ranges::default();
            if let Some(root) = anc.first() {
                v.visit(root);
            }
            v.0.sort_by_key(|&(_, e)| e);
            cx.state::<State>().ranges = Some((v.0.iter().map(|&(_, e)| e).collect(), v.0));
        }
        let (ends, ranges) = cx.state::<State>().ranges.take().unwrap();
        let offense = self.offense(cx, node, &name, body, &ranges, &ends);
        cx.state::<State>().ranges = Some((ends, ranges));
        let Some(msg) = offense else { return };
        let start = node.location().start_offset();
        cx.add(start, name.location().end_offset(), msg);
    }
}

impl Documentation {
    fn offense(&self, cx: &Cx, node: &Node, name: &Node, body: Option<Node>, ranges: &[(usize, usize)], ends: &[usize]) -> Option<String> {
        let src = cx.src;
        let anc = cx.ancestors;
        let start = node.location().start_offset();
        // documentation_comment? (`ends` are the sorted node ends, as in `ranges`)
        let r = range(node);
        let enclosing = enclosing_start(anc, r.0);
        // As the first statement of a body with `rescue`/`ensure`, the parser `rescue`/`ensure`
        // node starts where the class does and takes the comments before it.
        let wrapped = matches!(anc, [.., b, s]
            if s.as_statements_node().and_then(|s| s.body().iter().next()).is_some_and(|f| range(&f) == r)
                && b.as_begin_node().is_some_and(|b| b.rescue_clause().is_some() || b.ensure_clause().is_some()));
        if !wrapped && !comments_taken_by_ancestor(anc, start) && documentation_comment(cx, ends, start, enclosing) {
            return None;
        }
        // constant_allowed?
        let short = match name {
            Node::ConstantReadNode { .. } => name.as_constant_read_node().unwrap().name().as_slice().to_vec(),
            _ => name.as_constant_path_node().and_then(|p| p.name()).map(|n| n.as_slice().to_vec()).unwrap_or_default(),
        };
        if cx.cfg.strings("AllowedConstants").iter().any(|c| c.as_bytes() == short.as_slice()) {
            return None;
        }
        // nodoc_self_or_outer_module?
        if nodoc_here(src, ranges, range(name), start, false) || nodoc_up(src, ranges, anc, anc.len()) {
            return None;
        }
        let name_src = src.slice(range(name).0, range(name).1);
        if name_src.contains("::") {
            let mut search = OuterModule::default();
            search.visit(node);
            if let Some((s, e, ps, pe)) = search.0 {
                // nodoc_comment?(const): its namespace's comment on the const's line, then the
                // parents (consts within the name, then the class and its ancestors).
                if nodoc_here(src, ranges, (ps, pe), s, false) {
                    return None;
                }
                let name_r = range(name);
                if s >= name_r.0 && e <= name_r.1 {
                    // Outer consts of the name: each one's namespace ends at the inner const.
                    let mut outer = name.as_constant_path_node();
                    let mut chain = Vec::new();
                    while let Some(p) = outer {
                        let r = range(&p.as_node());
                        if r.1 <= e {
                            break;
                        }
                        chain.push((p.parent().map(|q| range(&q)), r.0));
                        outer = p.parent().and_then(|q| q.as_constant_path_node());
                    }
                    for (first, at) in chain.into_iter().rev() {
                        if first.is_some_and(|f| nodoc_here(src, ranges, f, at, true)) {
                            return None;
                        }
                    }
                }
                if nodoc_here(src, ranges, range(name), start, true) || nodoc_up(src, ranges, anc, anc.len()) {
                    return None;
                }
            }
        }
        // include_statement_only?
        if body.as_ref().is_some_and(include_statement_only) {
            return None;
        }
        self.message(cx, node, is_class(node))
    }

    fn message(&self, cx: &Cx, node: &Node, class: bool) -> Option<String> {
        let src = cx.src;
        let mut parts = Vec::new();
        for n in cx.ancestors.iter().chain(std::iter::once(node)) {
            match n {
                Node::ClassNode { .. } => qualify_const(src, &n.as_class_node().unwrap().constant_path(), &mut parts),
                Node::ModuleNode { .. } => qualify_const(src, &n.as_module_node().unwrap().constant_path(), &mut parts),
                _ => {}
            }
        }
        let identifier = parts.join("::").replacen("::::", "::", 1);
        let kind = if class { "class" } else { "module" };
        Some(format!("Missing top-level documentation comment for `{kind} {identifier}`."))
    }
}

fn is_class(node: &Node) -> bool {
    node.as_class_node().is_some()
}
