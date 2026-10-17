use crate::prelude::*;
use helpers::range_help::{range_with_surrounding_space, Side, Surrounding};

pub const NAME: &str = "Lint/Void";
pub static COP: Void = Void;
pub struct Void;

const UNARY_OPERATORS: &[&str] = &["+@", "-@", "~", "!"];
const BINARY_OPERATORS: &[&str] = &["*", "/", "%", "+", "-", "==", "===", "!=", "<", ">", "<=", ">=", "<=>"];
const NONMUTATING_METHODS: &[&str] = &[
    "capitalize", "chomp", "chop", "compact", "delete_prefix", "delete_suffix", "downcase", "encode", "flatten",
    "gsub", "lstrip", "merge", "next", "reject", "reverse", "rotate", "rstrip", "scrub", "select", "shuffle", "slice",
    "sort", "sort_by", "squeeze", "strip", "sub", "succ", "swapcase", "tr", "tr_s", "transform_values",
    "unicode_normalize", "uniq", "upcase", "collect", "map",
];

impl Cop for Void {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::STATEMENTS_NODE,
            kind::PARENTHESES_NODE,
            kind::EMBEDDED_STATEMENTS_NODE,
            kind::BEGIN_NODE,
            kind::CALL_NODE,
            kind::ENSURE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node {
            Node::CallNode { .. } => on_block(cx, node),
            Node::EnsureNode { .. } => {
                // on_ensure: a single-statement ensure body (a `begin` is handled by on_begin).
                let Some(s) = node.as_ensure_node().unwrap().statements() else { return };
                let body: Vec<Node> = s.body().iter().collect();
                if let [only] = body.as_slice()
                    && only.as_parentheses_node().is_none() {
                        let ctx = Ctx::new(cx, only, &[node]);
                        check_expression(cx, &ctx, only);
                    }
            }
            _ => {
                if let Some(children) = begin_children(node, cx.ancestors) {
                    check_begin(cx, node, children);
                }
            }
        }
    }
}

/// The children of a parser `begin`/`kwbegin`, or None if `node` is not one.
fn begin_children<'pr>(node: &Node<'pr>, ancestors: &[Node<'pr>]) -> Option<Vec<Node<'pr>>> {
    let statements = |s: Option<ruby_prism::StatementsNode<'pr>>| s.map_or(Vec::new(), |s| s.body().iter().collect());
    match node {
        Node::StatementsNode { .. } => {
            let body: Vec<Node> = node.as_statements_node().unwrap().body().iter().collect();
            if body.len() < 2 {
                return None;
            }
            match ancestors.last() {
                // The parentheses / interpolation / kwbegin is the begin itself.
                Some(Node::ParenthesesNode { .. }) | Some(Node::EmbeddedStatementsNode { .. }) => None,
                Some(p @ Node::BeginNode { .. }) if is_plain_kwbegin(p) => None,
                _ => Some(body),
            }
        }
        Node::ParenthesesNode { .. } => {
            // `def (recv).foo`: parser drops these parentheses.
            if ancestors.last().and_then(|p| p.as_def_node()).and_then(|d| d.receiver()).is_some_and(|r| {
                r.location().start_offset() == node.location().start_offset()
            }) {
                return None;
            }
            Some(match node.as_parentheses_node().unwrap().body() {
                None => Vec::new(),
                Some(b) => match b.as_statements_node() {
                    Some(s) => s.body().iter().collect(),
                    None => vec![b],
                },
            })
        }
        Node::EmbeddedStatementsNode { .. } => Some(statements(node.as_embedded_statements_node().unwrap().statements())),
        Node::BeginNode { .. } if is_plain_kwbegin(node) => Some(statements(node.as_begin_node().unwrap().statements())),
        _ => None,
    }
}

/// `begin ... end` without rescue/ensure: parser `(kwbegin stmts...)`. (With them, its only child
/// is the rescue/ensure node, which this cop never reports.)
fn is_plain_kwbegin(node: &Node) -> bool {
    node.as_begin_node().is_some_and(|b| {
        b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none()
    })
}

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// What the checks need to know about a node's surroundings.
struct Ctx {
    /// The parser parent is a void context (def initialize/setter, for, each/tap block, ensure)
    /// and the node is its last child.
    void: bool,
    /// The parser parent is a setter `def`.
    setter_parent: bool,
    /// The parser parent is if/case/when/case_match/in_pattern.
    branch_parent: bool,
    /// Inside a setter method (each_ancestor(:any_def).first.assignment_method?).
    in_setter_def: bool,
    /// The nearest block ancestor calls `each`.
    inside_each_block: bool,
}

fn setter_name(name: &str) -> bool {
    name.ends_with('=') && !matches!(name, "==" | "===" | "!=" | "<=" | ">=")
}

fn block_name<'pr>(block: &Node<'pr>, owner: Option<&Node<'pr>>) -> &'pr str {
    if block.as_lambda_node().is_some() {
        return "lambda";
    }
    match owner.and_then(|o| o.as_call_node()) {
        Some(c) => method_name(&c),
        None => "super",
    }
}

impl Ctx {
    /// `extra` are Prism ancestors below `cx.ancestors` (nearest last), leading to `node`.
    fn new(cx: &Cx, node: &Node, extra: &[&Node]) -> Ctx {
        let chain: Vec<&Node> = cx.ancestors.iter().chain(extra.iter().copied()).collect();
        let mut ctx = Ctx {
            void: false,
            setter_parent: false,
            branch_parent: false,
            in_setter_def: false,
            inside_each_block: false,
        };
        // Parser parent: the nearest non-transparent Prism ancestor, and the child we came from.
        let mut child = span(node);
        let mut parent_found = false;
        let mut block_found = false;
        for i in (0..chain.len()).rev() {
            let a = chain[i];
            if !parent_found {
                let transparent = match a {
                    Node::StatementsNode { .. } => {
                        let n = a.as_statements_node().unwrap().body().len();
                        n == 1
                            || matches!(chain.get(i.wrapping_sub(1)), Some(Node::ParenthesesNode { .. }) | Some(Node::EmbeddedStatementsNode { .. }))
                            || chain.get(i.wrapping_sub(1)).is_some_and(|p| is_plain_kwbegin(p))
                    }
                    Node::ElseNode { .. } | Node::ArgumentsNode { .. } | Node::ProgramNode { .. } => true,
                    _ => false,
                };
                if !transparent {
                    parent_found = true;
                    match a {
                        Node::DefNode { .. } => {
                            let d = a.as_def_node().unwrap();
                            let name = std::str::from_utf8(d.name().as_slice()).unwrap_or("");
                            let is_body = d.body().is_some_and(|b| span(&b) == child);
                            ctx.setter_parent = setter_name(name);
                            ctx.void = is_body && ((d.receiver().is_none() && name == "initialize") || setter_name(name));
                        }
                        Node::ForNode { .. } => {
                            ctx.void = a.as_for_node().unwrap().statements().is_some_and(|s| span(&s.as_node()) == child);
                        }
                        Node::BlockNode { .. } => {
                            let b = a.as_block_node().unwrap();
                            let is_body = b.body().is_some_and(|b| span(&b) == child);
                            ctx.void = is_body && matches!(block_name(a, i.checked_sub(1).map(|p| chain[p])), "each" | "tap");
                        }
                        Node::EnsureNode { .. } => {
                            ctx.void = a.as_ensure_node().unwrap().statements().is_some_and(|s| span(&s.as_node()) == child);
                        }
                        Node::IfNode { .. } | Node::UnlessNode { .. } | Node::CaseNode { .. } | Node::WhenNode { .. } => {
                            ctx.branch_parent = true;
                        }
                        Node::CaseMatchNode { .. } | Node::InNode { .. } => ctx.branch_parent = true,
                        _ => {}
                    }
                }
            }
            if !block_found {
                match a {
                    Node::BlockNode { .. } | Node::LambdaNode { .. } => {
                        block_found = true;
                        ctx.inside_each_block = block_name(a, i.checked_sub(1).map(|p| chain[p])) == "each";
                    }
                    // Inside the receiver/arguments of a call that has a block: parser's block wraps the send.
                    Node::CallNode { .. } => {
                        let c = a.as_call_node().unwrap();
                        let from_block = chain.get(i + 1).is_some_and(|n| n.as_block_node().is_some());
                        if !from_block && c.block().is_some_and(|b| b.as_block_node().is_some()) {
                            block_found = true;
                            ctx.inside_each_block = method_name(&c) == "each";
                        }
                    }
                    _ => {}
                }
            }
            if let Node::DefNode { .. } = a {
                let name = std::str::from_utf8(a.as_def_node().unwrap().name().as_slice()).unwrap_or("");
                ctx.in_setter_def = setter_name(name);
                break;
            }
            child = span(a);
        }
        ctx
    }
}

fn check_begin(cx: &mut Cx, node: &Node, mut expressions: Vec<Node>) {
    let ctx = Ctx::new(cx, node, &[]);
    if !ctx.void || ctx.inside_each_block || ctx.setter_parent {
        expressions.pop();
    }
    for expr in &expressions {
        let child_ctx = Ctx::new(cx, expr, &[node]);
        check_void_op(cx, expr, ctx.inside_each_block);
        check_expression(cx, &child_ctx, expr);
    }
}

fn on_block(cx: &mut Cx, node: &Node) {
    let call = node.as_call_node().unwrap();
    let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
    let name = method_name(&call);
    // in_void_context?(body): each/tap; `each` itself is skipped.
    if name != "tap" {
        return;
    }
    let Some(body) = block.body() else { return };
    let body = match body.as_statements_node() {
        Some(s) => {
            let stmts: Vec<Node> = s.body().iter().collect();
            if stmts.len() != 1 {
                return;
            }
            stmts.into_iter().next().unwrap()
        }
        None => body,
    };
    if body.as_parentheses_node().is_some() {
        return;
    }
    let block_node = block.as_node();
    let ctx = Ctx::new(cx, &body, &[node, &block_node]);
    check_void_op(cx, &body, false);
    check_expression(cx, &ctx, &body);
}

fn check_void_op(cx: &mut Cx, node: &Node, inside_each_block: bool) {
    // node = node.children.first while node&.begin_type?
    let mut node_owned;
    let mut node = node;
    while let Some(p) = node.as_parentheses_node() {
        let Some(b) = p.body() else { return };
        node_owned = match b.as_statements_node() {
            Some(s) => s.body().iter().next().unwrap(),
            None => b,
        };
        node = &node_owned;
    }
    let Some(call) = node.as_call_node() else { return };
    if call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return;
    }
    let name = method_name(&call);
    let unary = UNARY_OPERATORS.contains(&name);
    if !unary && !BINARY_OPERATORS.contains(&name) {
        return;
    }
    let args = send_args(&call);
    if !unary && call.call_operator_loc().is_some() && args.is_empty() {
        return;
    }
    if inside_each_block {
        return;
    }
    let Some(sel) = call.message_loc() else { return };
    let (ss, se) = (sel.start_offset(), sel.end_offset());
    let msg = format!("Operator `{name}` used in void context.");
    let range = span(node);
    let receiver_src = call.receiver().map(|r| node_text(&r).to_string());
    let dot = call.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()));
    let around = range_with_surrounding_space(
        cx.src.text,
        ss,
        se,
        Surrounding { side: Side::Both, newlines: false, ..Default::default() },
    );
    cx.add_fix(ss, se, msg, |c| {
        if args.is_empty() {
            if let Some(r) = &receiver_src {
                c.replace(range.0, range.1, r);
            }
        } else {
            if let Some((ds, de)) = dot {
                c.remove(ds, de);
            }
            c.replace(around.0, around.1, "\n");
        }
    });
}

fn check_expression(cx: &mut Cx, ctx: &Ctx, expr: &Node) {
    match expr {
        Node::IfNode { .. } | Node::UnlessNode { .. } => {
            // check_if_expression: the if-branch body only.
            let statements = match expr {
                Node::IfNode { .. } => expr.as_if_node().unwrap().statements(),
                _ => expr.as_unless_node().unwrap().statements(),
            };
            if let Some(body) = single(statements) {
                let ctx = Ctx { branch_parent: true, ..*ctx };
                check_void_expression_nodes(cx, &ctx, &body);
            }
        }
        Node::CaseNode { .. } => {
            let case = expr.as_case_node().unwrap();
            for w in case.conditions().iter() {
                if let Some(body) = single(w.as_when_node().unwrap().statements()) {
                    check_expression(cx, &Ctx { branch_parent: true, ..*ctx }, &body);
                }
            }
            if let Some(body) = single(case.else_clause().and_then(|e| e.statements())) {
                check_expression(cx, &Ctx { branch_parent: true, ..*ctx }, &body);
            }
        }
        Node::CaseMatchNode { .. } => {
            let case = expr.as_case_match_node().unwrap();
            for w in case.conditions().iter() {
                if let Some(body) = single(w.as_in_node().unwrap().statements()) {
                    check_expression(cx, &Ctx { branch_parent: true, ..*ctx }, &body);
                }
            }
            if let Some(body) = single(case.else_clause().and_then(|e| e.statements())) {
                check_expression(cx, &Ctx { branch_parent: true, ..*ctx }, &body);
            }
        }
        _ => check_void_expression_nodes(cx, ctx, expr),
    }
}

/// A branch body that is a single parser node (a multi-statement `begin` is never reported here).
fn single<'pr>(s: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Node<'pr>> {
    let body: Vec<Node> = s?.body().iter().collect();
    if body.len() == 1 { body.into_iter().next() } else { None }
}

impl Clone for Ctx {
    fn clone(&self) -> Self {
        *self
    }
}
impl Copy for Ctx {}

fn check_void_expression_nodes(cx: &mut Cx, ctx: &Ctx, expr: &Node) {
    check_literal(cx, ctx, expr);
    check_var(cx, ctx, expr);
    if expr.as_self_node().is_some() {
        report(cx, ctx, expr, "`self` used in void context.".to_string(), true);
    }
    if expr.as_defined_node().is_some() || helpers::node_types::lambda_or_proc(expr) {
        let msg = format!("`{}` used in void context.", node_text(expr));
        report(cx, ctx, expr, msg, true);
    }
    if cx.cfg.bool("CheckForMethodsWithNoSideEffects").unwrap_or(false) {
        check_nonmutating(cx, expr);
    }
}

/// add_offense(node) { autocorrect_void_expression }: remove the node and the space before it.
fn report(cx: &mut Cx, ctx: &Ctx, node: &Node, msg: String, correct: bool) {
    let (s, e) = span(node);
    let correct = correct && !ctx.branch_parent && !ctx.in_setter_def;
    let (rs, re) = range_with_surrounding_space(cx.src.text, s, e, Surrounding { side: Side::Left, ..Default::default() });
    cx.add_fix(s, e, msg, |c| {
        if correct {
            c.remove(rs, re);
        }
    });
}

fn check_var(cx: &mut Cx, ctx: &Ctx, node: &Node) {
    match node {
        Node::LocalVariableReadNode { .. }
        | Node::InstanceVariableReadNode { .. }
        | Node::GlobalVariableReadNode { .. }
        | Node::ClassVariableReadNode { .. }
        | Node::ItLocalVariableReadNode { .. }
        | Node::BackReferenceReadNode { .. }
        | Node::NumberedReferenceReadNode { .. } => {
            if matches!(node, Node::BackReferenceReadNode { .. } | Node::NumberedReferenceReadNode { .. })
                || helpers::node_types::is_legacy_numbered_read(cx, node)
            {
                return; // nth_ref / back_ref are not `variable?`, nor a `_1` send
            }
            let msg = format!("Variable `{}` used in void context.", node_text(node));
            report(cx, ctx, node, msg, true);
        }
        Node::SourceEncodingNode { .. } => {
            let msg = format!("Variable `{}` used in void context.", node_text(node));
            report(cx, ctx, node, msg, true);
        }
        Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => {
            let msg = format!("Constant `{}` used in void context.", node_text(node));
            report(cx, ctx, node, msg, false);
        }
        _ => {}
    }
}

fn check_literal(cx: &mut Cx, ctx: &Ctx, node: &Node) {
    if !entirely_literal(node)
        || matches!(
            node,
            Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. } | Node::RangeNode { .. } | Node::NilNode { .. }
        )
    {
        return;
    }
    let msg = format!("Literal `{}` used in void context.", node_text(node));
    report(cx, ctx, node, msg, true);
}

fn entirely_literal(node: &Node) -> bool {
    match node {
        Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().iter().all(|e| entirely_literal(&e)),
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().all(|e| match e.as_assoc_node() {
            Some(p) => entirely_literal(&p.key()) && entirely_literal(&p.value()),
            None => true, // each_key / each_value skip `**kwsplat`
        }),
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            method_name(&c) == "freeze"
                && c.block().is_none_or(|b| b.as_block_node().is_none())
                && c.receiver().is_some_and(|r| entirely_literal(&r))
        }
        _ => helpers::literal_node::literal(node),
    }
}

fn check_nonmutating(cx: &mut Cx, node: &Node) {
    let Some(call) = node.as_call_node() else { return };
    let name = method_name(&call);
    if !NONMUTATING_METHODS.contains(&name) {
        return;
    }
    let suggestion = if name == "collect" || name == "map" { "each".to_string() } else { format!("{name}!") };
    let msg = format!("Method `#{name}` used in void context. Did you mean `#{suggestion}`?");
    let (s, e) = span(node);
    let Some(sel) = call.message_loc() else { return };
    let (ss, se) = (sel.start_offset(), sel.end_offset());
    cx.add_fix(s, e, msg, |c| c.replace(ss, se, &suggestion));
}
