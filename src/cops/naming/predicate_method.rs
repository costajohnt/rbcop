use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Naming/PredicateMethod";
pub static COP: PredicateMethod = PredicateMethod;
pub struct PredicateMethod;

const MSG_PREDICATE: &str = "Predicate method names should end with `?`.";
const MSG_NON_PREDICATE: &str = "Non-predicate method names should not end with `?`.";

/// A return value: a parser node, or the cop's synthetic `s(:nil)` / `s(:array)`.
enum Value<'pr> {
    Node(Node<'pr>),
    Nil,
    Array,
}

/// The parser node for a Prism body: a one-statement StatementsNode is just its statement.
fn resolve(node: Node) -> Node {
    if let Some(s) = node.as_statements_node() {
        let mut body = s.body().iter();
        if let (Some(only), None) = (body.next(), body.next()) {
            return only;
        }
    }
    node
}

/// Children of a parser `begin` (a 2+ statement body or parentheses), None for other nodes.
fn begin_children<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    if let Some(s) = node.as_statements_node() {
        return Some(s.body().iter().collect());
    }
    let p = node.as_parentheses_node()?;
    Some(match p.body() {
        None => Vec::new(),
        Some(b) => match b.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![b],
        },
    })
}

/// `extract_return_value`
fn extract(node: Node) -> Value {
    let Some(ret) = node.as_return_node() else { return Value::Node(node) };
    let args: Vec<Node> = ret.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
    match args.len() {
        0 => Value::Nil,
        1 => Value::Node(args.into_iter().next().unwrap()),
        _ => Value::Array,
    }
}

/// `last_value`
fn last_value(node: Node) -> Value {
    let node = resolve(node);
    match begin_children(&node) {
        Some(children) => children.into_iter().last().map_or(Value::Nil, extract),
        None => extract(node),
    }
}

fn branch_value(branch: Option<Node>) -> Value {
    branch.map_or(Value::Nil, last_value)
}

struct Returns<'pr>(Vec<Node<'pr>>);

impl<'pr> Visit<'pr> for Returns<'pr> {
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode<'pr>) {
        self.0.push(node.as_node());
        ruby_prism::visit_return_node(self, node);
    }
}

fn return_values<'pr>(def: &ruby_prism::DefNode<'pr>) -> Vec<Value<'pr>> {
    let body = || resolve(def.body().unwrap());
    let mut values = Vec::new();
    if begin_children(&body()).is_none() {
        values.push(extract(body()));
    }
    let mut returns = Returns(Vec::new());
    returns.visit(&body());
    values.extend(returns.0.into_iter().map(extract));
    values.push(last_value(body()));
    process(values)
}

fn process(values: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    for v in values {
        match v {
            Value::Node(n) if is_conditional(&n) => out.extend(process(conditional_branches(&n))),
            Value::Node(n) if n.as_and_node().is_some() || n.as_or_node().is_some() => {
                let mut clauses = Vec::new();
                and_or_clauses(n, &mut clauses);
                out.extend(process(clauses.into_iter().map(Value::Node).collect()));
            }
            v => out.push(v),
        }
    }
    out
}

fn and_or_clauses<'pr>(node: Node<'pr>, out: &mut Vec<Node<'pr>>) {
    if let Some(a) = node.as_and_node() {
        and_or_clauses(a.left(), out);
        and_or_clauses(a.right(), out);
    } else if let Some(o) = node.as_or_node() {
        and_or_clauses(o.left(), out);
        and_or_clauses(o.right(), out);
    } else {
        out.push(node);
    }
}

/// `conditional?`: if while until case case_match (not `begin..end while`, parser `while_post`).
fn is_conditional(node: &Node) -> bool {
    match node {
        Node::IfNode { .. } | Node::UnlessNode { .. } | Node::CaseNode { .. } | Node::CaseMatchNode { .. } => true,
        Node::WhileNode { .. } => !node.as_while_node().unwrap().is_begin_modifier(),
        Node::UntilNode { .. } => !node.as_until_node().unwrap().is_begin_modifier(),
        _ => false,
    }
}

/// rubocop-ast `IfNode#branches` (raw bodies) for an `if`/ternary/elsif node.
fn if_branches<'pr>(node: &ruby_prism::IfNode<'pr>) -> Vec<Option<Node<'pr>>> {
    let mut out = vec![node.statements().map(|s| s.as_node())];
    match node.subsequent() {
        None => {}
        Some(sub) => match sub.as_if_node() {
            Some(elsif) => out.extend(if_branches(&elsif)),
            None => out.push(sub.as_else_node().unwrap().statements().map(|s| s.as_node())),
        },
    }
    out
}

/// `extract_conditional_branches`
fn conditional_branches<'pr>(node: &Node<'pr>) -> Vec<Value<'pr>> {
    let loop_body = match node {
        Node::WhileNode { .. } => Some(node.as_while_node().unwrap().statements()),
        Node::UntilNode { .. } => Some(node.as_until_node().unwrap().statements()),
        _ => None,
    };
    if let Some(body) = loop_body {
        return vec![branch_value(body.map(|s| s.as_node()))];
    }
    let (branches, has_else_branch) = match node {
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            let else_branch = n.subsequent().is_some_and(|s| s.as_else_node().is_none_or(|e| e.statements().is_some()));
            (if_branches(&n), else_branch)
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            let mut b = vec![n.statements().map(|s| s.as_node())];
            let els = n.else_clause().map(|e| e.statements().map(|s| s.as_node()));
            let has = els.as_ref().is_some_and(|e| e.is_some());
            b.extend(els);
            (b, has)
        }
        Node::CaseNode { .. } => {
            let n = node.as_case_node().unwrap();
            let mut b: Vec<Option<Node>> =
                n.conditions().iter().map(|w| w.as_when_node().unwrap().statements().map(|s| s.as_node())).collect();
            let els = n.else_clause().map(|e| e.statements().map(|s| s.as_node()));
            let has = els.as_ref().is_some_and(|e| e.is_some());
            b.extend(els);
            (b, has)
        }
        _ => {
            let n = node.as_case_match_node().unwrap();
            let mut b: Vec<Option<Node>> =
                n.conditions().iter().map(|i| i.as_in_node().unwrap().statements().map(|s| s.as_node())).collect();
            // An empty `else` is parser `empty_else`: a nil body, but a truthy else_branch.
            let els = n.else_clause().map(|e| e.statements().map(|s| s.as_node()));
            let has = els.is_some();
            b.extend(els);
            (b, has)
        }
    };
    let mut out: Vec<Value> = branches.into_iter().map(branch_value).collect();
    if !has_else_branch {
        out.push(Value::Nil);
    }
    out
}

/// A parser `send`/`csend` (a call with a `{ }`/`do` block is a parser `block`; `/re/ =~ x` is
/// `match_with_lvasgn`).
fn as_call<'pr>(value: &Value<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    let Value::Node(n) = value else { return None };
    let call = n.as_call_node()?;
    if call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    if method_name(&call) == "=~" && call.receiver().is_some_and(|r| r.as_regular_expression_node().is_some()) {
        return None;
    }
    Some(call)
}

fn is_super(value: &Value) -> bool {
    let Value::Node(n) = value else { return false };
    match n {
        Node::SuperNode { .. } => n.as_super_node().unwrap().block().is_none_or(|b| b.as_block_node().is_none()),
        Node::ForwardingSuperNode { .. } => n.as_forwarding_super_node().unwrap().block().is_none(),
        _ => false,
    }
}

fn is_boolean(value: &Value) -> bool {
    matches!(value, Value::Node(Node::TrueNode { .. } | Node::FalseNode { .. }))
}

fn is_literal(value: &Value) -> bool {
    match value {
        Value::Node(n) => helpers::literal_node::literal(n),
        Value::Nil | Value::Array => true,
    }
}

struct Settings {
    conservative: bool,
    wayward: Vec<String>,
}

impl Settings {
    /// `method_returning_boolean?`
    fn method_returning_boolean(&self, value: &Value) -> bool {
        let Some(call) = as_call(value) else { return false };
        let name = method_name(&call);
        if self.wayward.iter().any(|w| w == name) {
            return false;
        }
        matches!(name, "==" | "===" | "!=" | "<=" | ">=" | ">" | "<")
            || name.ends_with('?')
            || (name == "!" && call.receiver().is_some())
    }

    fn boolean_return(&self, value: &Value) -> bool {
        is_boolean(value) || self.method_returning_boolean(value)
    }

    /// `acceptable?`
    fn acceptable(&self, values: &[Value]) -> bool {
        self.conservative
            && values.iter().any(|v| is_super(v) || (as_call(v).is_some() && !self.method_returning_boolean(v)))
    }

    fn all_boolean(&self, values: &[Value]) -> bool {
        let mut values = values.iter().filter(|v| !is_super(v)).peekable();
        values.peek().is_some() && values.all(|v| self.boolean_return(v))
    }

    fn potential_non_predicate(&self, values: &[Value]) -> bool {
        if self.conservative && values.iter().any(|v| self.boolean_return(v)) {
            return false;
        }
        values.iter().any(|v| is_literal(v) && !is_boolean(v))
    }
}

impl Cop for PredicateMethod {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
        let cfg = &cx.cfg;
        let allowed = name == "initialize"
            || helpers::allowed::allowed_method(cfg, name)
            || helpers::allowed::matches_allowed_pattern(cfg, name)
            || (cfg.bool("AllowBangMethods").unwrap_or(false) && name.ends_with('!'))
            || helpers::node_types::is_operator_method(name);
        if allowed || def.body().is_none() {
            return;
        }
        let settings = Settings {
            conservative: cfg.str("Mode").is_none_or(|m| m == "conservative"),
            wayward: cfg.strings("WaywardPredicates"),
        };
        let values = return_values(&def);
        if settings.acceptable(&values) {
            return;
        }
        let predicate = name.ends_with('?');
        let msg = if predicate && settings.potential_non_predicate(&values) {
            MSG_NON_PREDICATE
        } else if !predicate && settings.all_boolean(&values) {
            MSG_PREDICATE
        } else {
            return;
        };
        let l = def.name_loc();
        cx.add(l.start_offset(), l.end_offset(), msg);
    }
}
